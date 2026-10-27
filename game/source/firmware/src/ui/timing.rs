//! Timing: a zoomed tape with the error of each element over one of two views. BARS: session
//! histograms of mark and gap lengths with the averages under them. HAND: what your contacts do
//! around the keyer, for the keyer mode in use.

use core::fmt::Write;

use pocketcw_logic::{
    hand::{CLOSE_DOTS, RELEASE_RANGE},
    input::Action,
    keyer::Mode,
    settings::Settings,
    timing::{GAP_RANGE, Kind, MARK_RANGE, Moments, Quality},
};

use super::home::{number, units};
use super::{Ui, footer::FOOT_TOP, lanes, mode_name, settings::Section};
use crate::gfx::{TINY_W, rect, text_cols, tiny};
use crate::lcd::Lcd;
use crate::text::{Hundredths, Tenths, Text};
use crate::theme::{MAIN, quality_color};

const STATS_Y: i32 = 109;
const HIST_Y: i32 = 112;
const HIST_H: i32 = 26;
const HIST_W: i32 = 124;
/// Below the histograms' labels, so the last line ends above the footer.
const LINES_Y: i32 = HIST_Y + 14 + HIST_H + 16;
const LINE_STEP: i32 = 18;

impl Ui {
    pub(super) fn draw_timing(&mut self, lcd: &mut Lcd, s: &Settings) {
        self.header(lcd, s, "TIMING", &[mode_name(s.mode)]);
        self.draw_timing_keys(lcd);
        self.tape.clear(lcd, &self.theme, lanes(s));
        rect(lcd, 0, STATS_Y - 1, MAIN, 1, self.theme.rule());
        self.draw_timing_stats(lcd, s);
    }

    /// F3 names the view it switches to.
    fn draw_timing_keys(&self, lcd: &mut Lcd) {
        self.softkeys(lcd, ["SET", "", if self.timing_hand { "BARS" } else { "HAND" }, "BACK"]);
    }

    pub(super) fn draw_timing_stats(&self, lcd: &mut Lcd, s: &Settings) {
        match (self.timing_hand, s.mode) {
            (false, _) => self.draw_marks(lcd),
            (true, Mode::Straight) => self.draw_sides(lcd),
            (true, _) => self.draw_release(lcd),
        }
    }

    fn draw_marks(&self, lcd: &mut Lcd) {
        let st = &self.stats;
        let marks = |u| Quality::of(Kind::mark(u), u);
        let gaps = |u| Quality::of(Kind::gap(u), u);
        self.histogram(lcd, 4, HIST_W, "MARKS", &st.marks, MARK_RANGE, &[1.0, 3.0], marks);
        self.histogram(lcd, 144, HIST_W, "GAPS", &st.gaps, GAP_RANGE, &[1.0, 3.0, 7.0], gaps);

        let t = &self.theme;
        let mut line = Text::<32>::new();
        let _ = write!(line, "DIT ");
        ms(&mut line, &st.dit);
        let _ = write!(line, "  RATIO ");
        number(&mut line, st.ratio());
        text_cols(lcd, 0, LINES_Y, line.as_str(), super::COLS, t.fg, t.bg);

        line.clear();
        let _ = write!(line, "DAH ");
        ms(&mut line, &st.dah);
        let _ = write!(line, "  ");
        number(&mut line, st.effective_wpm());
        let _ = write!(line, " WPM");
        text_cols(lcd, 0, LINES_Y + LINE_STEP, line.as_str(), super::COLS, t.fg, t.bg);

        line.clear();
        let _ = write!(line, "GAP ");
        units(&mut line, st.element_gap.mean());
        let _ = write!(line, " CHR ");
        units(&mut line, st.char_gap.mean());
        let _ = write!(line, " WRD ");
        units(&mut line, st.word_gap.mean());
        text_cols(lcd, 0, LINES_Y + 2 * LINE_STEP, line.as_str(), super::COLS, t.fg, t.bg);
    }

    /// Iambic: how early you release before the keyer decides, and how often you press late.
    fn draw_release(&self, lcd: &mut Lcd) {
        let hand = &self.hand;
        let close = |dots| if dots < CLOSE_DOTS { Quality::Bad } else { Quality::Good };
        self.histogram(lcd, 4, 2 * HIST_W + 16, "RELEASE BEFORE THE KEYER DECIDES", &hand.release_bins, RELEASE_RANGE, &[1.0, 2.0], close);

        let t = &self.theme;
        let mut line = Text::<32>::new();
        let _ = write!(line, "RELEASE ");
        units(&mut line, hand.release.mean());
        let _ = write!(line, "  SD ");
        number(&mut line, hand.release.sd());
        text_cols(lcd, 0, LINES_Y, line.as_str(), super::COLS, t.fg, t.bg);

        line.clear();
        let _ = write!(line, "CLOSE ");
        percent(&mut line, hand.close, hand.release.count);
        let _ = write!(line, " OF {}", hand.release.count);
        text_cols(lcd, 0, LINES_Y + LINE_STEP, line.as_str(), super::COLS, t.fg, t.bg);

        line.clear();
        let _ = write!(line, "LATE ");
        percent(&mut line, hand.late.count, hand.element_gaps);
        let _ = write!(line, " OF {}", hand.element_gaps);
        if let Some(late) = hand.late.mean() {
            let _ = write!(line, "  +{}U", Tenths(late));
        }
        text_cols(lcd, 0, LINES_Y + 2 * LINE_STEP, line.as_str(), super::COLS, t.fg, t.bg);
    }

    /// Straight key or sideswiper: each contact's dits and dahs, and the weight of dits.
    fn draw_sides(&self, lcd: &mut Lcd) {
        let t = &self.theme;
        text_cols(lcd, 0, HIST_Y, "     DIT        DAH", super::COLS, t.muted, t.bg);
        let mut line = Text::<32>::new();
        for (i, (name, side)) in [("TIP", &self.hand.sides[0]), ("RING", &self.hand.sides[1])].into_iter().enumerate() {
            line.clear();
            let _ = write!(line, "{name:<5}");
            ms(&mut line, &side.dit);
            while line.len() < 16 {
                let _ = line.write_char(' ');
            }
            ms(&mut line, &side.dah);
            text_cols(lcd, 0, HIST_Y + (i as i32 + 1) * LINE_STEP, line.as_str(), super::COLS, t.fg, t.bg);
        }

        line.clear();
        let weight = &self.hand.weight;
        let _ = match (weight.mean(), weight.sd()) {
            (Some(mean), Some(sd)) => write!(line, "WEIGHT {}  SD {}", Hundredths(mean), Hundredths(sd)),
            _ => write!(line, "WEIGHT --"),
        };
        text_cols(lcd, 0, LINES_Y, line.as_str(), super::COLS, t.fg, t.bg);

        line.clear();
        let [tip, ring] = &self.hand.sides;
        let _ = write!(line, "RATIO TIP ");
        number(&mut line, tip.ratio());
        let _ = write!(line, "  RING ");
        number(&mut line, ring.ratio());
        text_cols(lcd, 0, LINES_Y + LINE_STEP, line.as_str(), super::COLS, t.fg, t.bg);

        line.clear();
        let marks: u32 = [tip, ring].iter().map(|side| side.dit.count + side.dah.count).sum();
        let _ = write!(line, "OVERLAP ");
        percent(&mut line, self.hand.overlap, marks);
        let _ = write!(line, " OF {marks}");
        text_cols(lcd, 0, LINES_Y + 2 * LINE_STEP, line.as_str(), super::COLS, t.fg, t.bg);
    }

    /// Bars colored by `quality` at each bin's middle, with a dotted line at each of `marks`.
    /// Lengths are in dots, `range` across the `w` pixels.
    #[allow(clippy::too_many_arguments)]
    fn histogram(&self, lcd: &mut Lcd, x: i32, w: i32, title: &str, bins: &[u16], range: f32, marks: &[f32], quality: impl Fn(f32) -> Quality) {
        let t = &self.theme;
        rect(lcd, x, HIST_Y, w, HIST_H + 30, t.bg);
        tiny(lcd, x, HIST_Y, title, t.muted, t.bg);
        let top = HIST_Y + 14;
        let peak = bins.iter().copied().max().unwrap_or(0).max(1) as i32;
        for &target in marks {
            let tx = x + (target / range * w as f32) as i32;
            for y in (top..top + HIST_H).step_by(3) {
                rect(lcd, tx, y, 1, 1, t.muted);
            }
            let label = Text::<2>::format(format_args!("{}", target as i32));
            tiny(lcd, tx - TINY_W / 2, top + HIST_H + 2, label.as_str(), t.muted, t.bg);
        }
        let bin_w = w / bins.len() as i32;
        for (i, &count) in bins.iter().enumerate() {
            let h = count as i32 * HIST_H / peak;
            if h == 0 {
                continue;
            }
            let dots = (i as f32 + 0.5) * range / bins.len() as f32;
            rect(lcd, x + i as i32 * bin_w, top + HIST_H - h, bin_w - 1, h, quality_color(quality(dots)));
        }
        rect(lcd, x, top + HIST_H, w, 1, t.rule());
    }

    pub(super) fn timing_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        match action {
            Action::Soft(0) => self.open_settings_at(lcd, s, Section::Keyer),
            Action::Soft(2) => {
                self.timing_hand = !self.timing_hand;
                self.draw_timing_keys(lcd);
                rect(lcd, 0, STATS_Y, MAIN, FOOT_TOP - STATS_Y, self.theme.bg);
                self.draw_timing_stats(lcd, s);
            }
            Action::Soft(3) => self.back(lcd, s),
            _ => {}
        }
    }
}

/// A mean length in ms and its spread, or `--`.
fn ms<const N: usize>(out: &mut Text<N>, m: &Moments) {
    let _ = match (m.mean(), m.sd()) {
        (Some(mean), Some(sd)) => write!(out, "{}MS SD {}", mean as i32, sd as i32),
        _ => write!(out, "--"),
    };
}

fn percent<const N: usize>(out: &mut Text<N>, part: u32, whole: u32) {
    let _ = match whole {
        0 => write!(out, "--"),
        _ => write!(out, "{}%", (part * 100 + whole / 2) / whole),
    };
}
