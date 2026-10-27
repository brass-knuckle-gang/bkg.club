//! System: firmware and hardware details, startup check results, and live battery and supply
//! voltages.

use core::fmt::Write;

use pocketcw_logic::battery::{Charge, RECORD_LEN};
use pocketcw_logic::input::Action;
use pocketcw_logic::panel::{self, IdRead, Panel};

use super::{Screen, Ui, settings::Section};
use crate::battery::Reading;
use crate::codec::Codec;
use crate::gfx::{TINY_W, rect, text, text_right, tiny};
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::{BAD, BATTERY_SEGMENTS, Battery, MAIN};

const ROW_Y: i32 = 27;
const ROW_STEP: i32 = 18;

/// The screen's rows, top to bottom.
#[derive(Clone, Copy)]
enum Row {
    Firmware,
    Board,
    Mcu,
    Clock,
    Codec,
    Eeprom,
    Battery,
    Supply,
    Uptime,
    Boots,
}

const ROWS: [Row; 10] =
    [Row::Firmware, Row::Board, Row::Mcu, Row::Clock, Row::Codec, Row::Eeprom, Row::Battery, Row::Supply, Row::Uptime, Row::Boots];
/// Rows that change while the screen shows, redrawn at every poll.
const LIVE_ROWS: [Row; 3] = [Row::Battery, Row::Supply, Row::Uptime];

impl Row {
    fn label(self) -> &'static str {
        match self {
            Row::Firmware => "FIRMWARE",
            Row::Board => "BOARD",
            Row::Mcu => "MCU",
            Row::Clock => "CLOCK",
            Row::Codec => "CODEC",
            Row::Eeprom => "EEPROM",
            Row::Battery => "BATTERY",
            Row::Supply => "SUPPLY",
            Row::Uptime => "UPTIME",
            Row::Boots => "BOOTS",
        }
    }

    fn y(self) -> i32 {
        ROW_Y + self as i32 * ROW_STEP
    }
}

/// Where the panel type came from.
#[derive(Clone, Copy)]
pub enum PanelFrom {
    Chosen(panel::Source),
    /// F2 at power-on, but the EEPROM did not keep it.
    Unsaved,
}

impl PanelFrom {
    fn label(self) -> &'static str {
        match self {
            PanelFrom::Chosen(panel::Source::Board) => "AUTO",
            PanelFrom::Chosen(panel::Source::Id) => "ID",
            PanelFrom::Chosen(panel::Source::Saved) => "SET",
            PanelFrom::Unsaved => "NOSAVE",
        }
    }
}

/// What the startup steps found.
#[derive(Clone, Copy)]
pub struct SystemInfo {
    pub flash_kb: u16,
    /// Option byte Data1: the board revision after "2.", as hex digits.
    pub board: u8,
    pub panel: Panel,
    pub panel_from: PanelFrom,
    /// The panel's RDDID answer on PB4 and PB5.
    pub panel_id: [IdRead; 2],
    pub uid: [u32; 3],
    /// What feeds the PLL; None when the system is not on the PLL.
    pub clock: Option<crate::clock::Source>,
    /// None when no codec answered.
    pub codec: Option<Codec>,
    pub codec_ok: bool,
    pub eeprom_ok: bool,
    pub adc_ok: bool,
}

impl Ui {
    pub(super) fn draw_system(&mut self, lcd: &mut Lcd, s: &pocketcw_logic::settings::Settings) {
        let t = &self.theme;
        self.header(lcd, s, "SYSTEM", &[]);
        self.softkeys(lcd, ["SET", "", "", "BACK"]);
        for row in ROWS {
            text(lcd, 10, row.y(), row.label(), t.muted, t.bg);
            self.draw_system_value(lcd, row);
        }
        let below_rows = ROW_Y + ROWS.len() as i32 * ROW_STEP + 4;
        let mut lcd_id = Text::<28>::new();
        let _ = write!(lcd_id, "LCD");
        for (pin, read) in ["B4", "B5"].into_iter().zip(self.system.panel_id) {
            let _ = match read.driven() {
                Some(answer) => write!(lcd_id, " {pin} {answer:08X}"),
                None => write!(lcd_id, " {pin} FLOAT"),
            };
        }
        tiny(lcd, MAIN - 12 - lcd_id.len() as i32 * TINY_W, below_rows, lcd_id.as_str(), t.muted, t.bg);
        let uid = Text::<28>::format(format_args!("ID {:08X}{:08X}{:08X}", self.system.uid[0], self.system.uid[1], self.system.uid[2]));
        self.set_footer(lcd, uid.as_str());
    }

    fn draw_system_value(&self, lcd: &mut Lcd, row: Row) {
        let t = &self.theme;
        let info = &self.system;
        let ok_fail = |ok: bool| if ok { "OK" } else { "FAIL" };
        let mut value = Text::<24>::new();
        let mut ok = true;
        let _ = match row {
            Row::Firmware => write!(value, "v{}", env!("CARGO_PKG_VERSION")),
            Row::Board => {
                ok = !matches!(info.panel_from, PanelFrom::Unsaved);
                write!(value, "2.{:02X} {} {}", info.board, info.panel.name(), info.panel_from.label())
            }
            Row::Mcu => write!(value, "AT32F403A {}K", info.flash_kb),
            Row::Clock => {
                ok = info.clock == Some(crate::clock::Source::Crystal);
                let source = info.clock.map_or("FAIL", crate::clock::Source::name);
                write!(value, "{} MHZ {source}", crate::clock::SYSCLK_HZ / 1_000_000)
            }
            Row::Codec => {
                ok = info.codec_ok;
                match info.codec {
                    Some(codec) => write!(value, "{} {}", codec.name(), ok_fail(ok)),
                    None => write!(value, "NONE FOUND"),
                }
            }
            Row::Eeprom => {
                ok = info.eeprom_ok;
                write!(value, "24C16 {}", ok_fail(ok))
            }
            Row::Battery | Row::Supply if self.battery.is_none() => {
                ok = false;
                write!(value, "ADC FAIL")
            }
            Row::Battery => {
                let raw = self.battery.unwrap_or_default().raw;
                let mv = self.gauge.battery_mv().unwrap_or_default();
                ok = !self.gauge.low();
                let volts = |v: &mut Text<24>| write!(v, "{}.{:02}V", mv / 1000, mv % 1000 / 10);
                match self.gauge.charge() {
                    Some(Charge::Usb) => write!(value, "USB ").and_then(|_| volts(&mut value)),
                    Some(Charge::Percent(p)) => volts(&mut value).and_then(|_| write!(value, " {p}%")),
                    None => Ok(()),
                }
                .and_then(|_| write!(value, " {raw}"))
            }
            Row::Supply => {
                let mv = self.battery.unwrap_or_default().supply_mv;
                write!(value, "{}.{:02}V", mv / 1000, mv % 1000 / 10)
            }
            Row::Uptime => {
                let s = self.now_ms / 1000;
                write!(value, "{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
            }
            Row::Boots => write!(value, "{}", self.boot_count),
        };
        let y = row.y();
        let right = MAIN - 12;
        let width = value.len().max(15) as i32 * crate::gfx::CHAR_W;
        rect(lcd, right - width, y, width, 16, t.bg);
        text_right(lcd, right, y, value.as_str(), if ok { t.fg } else { BAD }, t.bg);
    }

    /// New battery reading and time, every second: updates the gauge, the header icon and the
    /// System screen.
    pub fn poll(&mut self, lcd: &mut Lcd, reading: Option<Reading>, now_ms: u32) {
        self.battery = reading;
        self.now_ms = now_ms;
        if self.listening {
            self.history.active(now_ms);
        }
        if let Some(r) = reading {
            let was_low = self.gauge.low();
            self.gauge.update(r.raw, r.supply_mv, now_ms);
            if self.gauge.low() && !was_low {
                self.status(lcd, "BATTERY LOW: PLUG IN USB");
            }
        }
        let icon = self.battery_icon();
        if icon != self.battery_shown && !self.full_screen() {
            self.battery_shown = icon;
            self.theme.battery(lcd, icon);
        }
        if self.screen == Screen::System {
            for row in LIVE_ROWS {
                self.draw_system_value(lcd, row);
            }
        }
    }

    pub(super) fn battery_icon(&self) -> Option<Battery> {
        Some(match self.gauge.charge()? {
            Charge::Usb => Battery::Usb,
            Charge::Percent(_) if self.gauge.low() => Battery::Segments(0),
            Charge::Percent(p) => Battery::Segments((p as u16 * BATTERY_SEGMENTS as u16).div_ceil(100).max(1) as u8),
        })
    }

    /// Empty for long enough that the device should power off.
    pub fn battery_empty(&self) -> bool {
        self.gauge.empty()
    }

    /// The battery calibration record, once after each change.
    pub fn take_battery(&mut self) -> Option<[u8; RECORD_LEN]> {
        self.gauge.take_record()
    }

    pub(super) fn system_act(&mut self, lcd: &mut Lcd, s: &mut pocketcw_logic::settings::Settings, action: Action) {
        if action == Action::Soft(0) {
            self.open_settings_at(lcd, s, Section::Theme);
        } else if action == Action::Soft(3) {
            self.back(lcd, s);
        }
    }
}
