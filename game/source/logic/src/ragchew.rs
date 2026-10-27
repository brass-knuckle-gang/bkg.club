//! Ragchew: two stations hold a whole contact for you to copy. One calls CQ, the other answers,
//! and they trade reports, names, QTHs, rigs, antennas, weather, years licensed, ages and jobs,
//! then sign with 73 and the last dits. Each over is split into lines short enough for the
//! player; the quiz asks about one fact per over.

use core::fmt::{self, Write};

use crate::drill::MAX_GROUP;
use crate::player::MAX_TEXT;
use crate::qso::{CALL_LEN, GREETINGS, Home, home, phrase};
use crate::rng::Rng;
use crate::text::Text;

/// Overs in one contact: CQ, answer, three exchanges each, two sign-offs, and the last dits.
pub const OVERS: usize = 11;
/// The last over: the station that called CQ answers the closing dits.
const DITS: usize = OVERS - 1;
pub const MAX_LINES: usize = 5;

const NAMES: [&str; 44] = [
    "BOB", "JIM", "TOM", "MIKE", "DAVE", "JOHN", "BILL", "STEVE", "ED", "AL", "RON", "DON", "GARY", "KEN", "JOE", "MATT",
    "TIM", "DAN", "BEN", "SUE", "ANN", "MARY", "LISA", "KAREN", "JEN", "KIM", "ROSE", "SARAH", "JAN", "AMY", "BETH", "EMMA",
    "KATE", "ANNA", "NINA", "MEG", "ZOE", "PAT", "CHRIS", "SAM", "ALEX", "MAX", "LEE", "RAY",
];

/// Cities and their states by call area digit, at most 10 letters without spaces for the quiz.
const CITIES_BY_AREA: [&[(&str, &str)]; 10] = [
    &[("DENVER", "CO"), ("WICHITA", "KS"), ("OMAHA", "NE"), ("FARGO", "ND")],
    &[("BOSTON", "MA"), ("PORTLAND", "ME"), ("BURLINGTON", "VT"), ("HARTFORD", "CT"), ("CONCORD", "NH")],
    &[("ALBANY", "NY"), ("BUFFALO", "NY"), ("TRENTON", "NJ"), ("NEWARK", "NJ")],
    &[("PITTSBURGH", "PA"), ("ERIE", "PA"), ("DOVER", "DE"), ("BALTIMORE", "MD"), ("WASHINGTON", "DC")],
    &[("RICHMOND", "VA"), ("RALEIGH", "NC"), ("ATLANTA", "GA"), ("TAMPA", "FL"), ("NASHVILLE", "TN"), ("MOBILE", "AL")],
    &[("AUSTIN", "TX"), ("TULSA", "OK"), ("JACKSON", "MS")],
    &[("FRESNO", "CA"), ("OAKLAND", "CA"), ("REDDING", "CA")],
    &[("SEATTLE", "WA"), ("EUGENE", "OR"), ("BOISE", "ID"), ("RENO", "NV"), ("TUCSON", "AZ"), ("BOZEMAN", "MT")],
    &[("DETROIT", "MI"), ("TOLEDO", "OH"), ("DAYTON", "OH"), ("WHEELING", "WV")],
    &[("CHICAGO", "IL"), ("PEORIA", "IL"), ("MADISON", "WI"), ("GARY", "IN")],
];

/// Rigs and the power they run.
const RIGS: [(&str, u8); 12] = [
    ("IC7300", 100),
    ("IC705", 10),
    ("FT991A", 100),
    ("FTDX10", 100),
    ("FT710", 100),
    ("FT891", 100),
    ("K3", 100),
    ("KX2", 10),
    ("KX3", 5),
    ("TS590", 100),
    ("QCX", 5),
    ("G90", 20),
];

/// Antennas and how high they go up, in feet; None for one sent without a height.
const ANTENNAS: [(&str, Option<(u8, u8)>); 10] = [
    ("DIPOLE", Some((20, 60))),
    ("EFHW", Some((20, 50))),
    ("VERT", None),
    ("YAGI", Some((30, 70))),
    ("HEXBEAM", Some((25, 45))),
    ("G5RV", Some((25, 50))),
    ("LOOP", Some((20, 50))),
    ("WINDOM", Some((25, 50))),
    ("BEAM", Some((30, 70))),
    ("LONGWIRE", None),
];
const WEATHER: [&str; 7] = ["SUNNY", "CLOUDY", "RAIN", "SNOW", "FOG", "WINDY", "CLEAR"];
const SNOW: &str = "SNOW";
/// Jobs a station works at, and, from 62, what a retired station did.
const JOBS: [&str; 4] = ["ENGINEER", "TEACHER", "DOCTOR", "PILOT"];
const RETIRED_FROM: [&str; 8] = ["ENGINEER", "TEACHER", "DOCTOR", "PILOT", "ARMY", "NAVY", "AIR FORCE", "POLICE"];
const RETIRE_AGE: u8 = 62;
/// Ragchew reports are honest numbers, not the contest 5NN.
const REPORTS: [&str; 9] = ["599", "589", "579", "579", "569", "559", "459", "449", "339"];

#[derive(Clone, Copy, Debug)]
pub struct Station {
    pub call: Text<CALL_LEN>,
    name: &'static str,
    city: &'static str,
    /// Empty for DX.
    state: &'static str,
    /// Empty in the US and Canada.
    country: &'static str,
    /// The report this station gives the other.
    report: &'static str,
    rig: &'static str,
    watts: u8,
    antenna: &'static str,
    /// 0 for an antenna sent without a height.
    height_ft: u8,
    weather: &'static str,
    temp: u8,
    /// Outside the US: temperatures in Celsius and heights in meters.
    metric: bool,
    years_licensed: u8,
    age: u8,
    job: &'static str,
    style: Style,
    /// Timing jitter in percent, used when the human fist setting is on.
    pub fist: u8,
    pub pitch_offset_hz: i16,
}

impl Station {
    fn new(rng: &mut Rng) -> Station {
        let home = home(rng);
        let call = home.call(rng);
        let (city, state, country) = match home {
            Home::Us(digit) => {
                let cities = CITIES_BY_AREA[digit as usize];
                let (city, state) = cities[rng.below(cities.len() as u32) as usize];
                (city, state, "")
            }
            Home::Abroad(abroad) => (abroad.cities[rng.below(abroad.cities.len() as u32) as usize], abroad.region, abroad.country),
        };
        let metric = matches!(home, Home::Abroad(_));
        let (rig, watts) = RIGS[rng.below(RIGS.len() as u32) as usize];
        let (antenna, heights) = ANTENNAS[rng.below(ANTENNAS.len() as u32) as usize];
        let height_ft = heights.map_or(0, |(low, high)| low + 5 * rng.below((high - low) as u32 / 5 + 1) as u8);
        let weather = WEATHER[rng.below(WEATHER.len() as u32) as usize];
        let temp = match (metric, weather == SNOW) {
            (false, true) => 15 + rng.below(18),
            (false, false) => 40 + rng.below(56),
            (true, true) => rng.below(3),
            (true, false) => 5 + rng.below(31),
        } as u8;
        let age = 25 + rng.below(60) as u8;
        let years_licensed = 2 + rng.below((age as u32 - 15).min(59)) as u8;
        let jobs: &[&str] = if age >= RETIRE_AGE { &RETIRED_FROM } else { &JOBS };
        Station {
            call,
            name: NAMES[rng.below(NAMES.len() as u32) as usize],
            city,
            state,
            country,
            report: REPORTS[rng.below(REPORTS.len() as u32) as usize],
            rig,
            watts,
            antenna,
            height_ft,
            weather,
            temp,
            metric,
            years_licensed,
            age,
            job: jobs[rng.below(jobs.len() as u32) as usize],
            style: Style::new(rng),
            fist: rng.below(13) as u8,
            pitch_offset_hz: (rng.below(21) as i16 - 10) * 10,
        }
    }
}

/// How a station words its overs, picked once so it stays the same through the contact.
#[derive(Clone, Copy, Debug)]
struct Style {
    /// CQ CQ CQ rather than CQ CQ.
    long_cq: bool,
    /// OP rather than NAME.
    op: bool,
    /// QTH NR rather than QTH.
    near: bool,
    qth_before_name: bool,
    /// Picks among the wordings of the rig, weather, licence and sign-off lines, and of the
    /// line that hands the contact back.
    rig: u8,
    wx: u8,
    ham: u8,
    bye: u8,
    hand_back: u8,
}

impl Style {
    fn new(rng: &mut Rng) -> Style {
        let mut flip = || rng.below(2) == 0;
        let (long_cq, op, near, qth_before_name) = (flip(), flip(), flip(), flip());
        let mut pick = |n: u32| rng.below(n) as u8;
        Style { long_cq, op, near, qth_before_name, rig: pick(3), wx: pick(3), ham: pick(2), bye: pick(3), hand_back: pick(3) }
    }
}

/// One over: who sends it and its lines.
#[derive(Clone, Copy, Debug)]
pub struct Over {
    /// Index into `Ragchew::stations`.
    pub from: usize,
    lines: [Text<MAX_TEXT>; MAX_LINES],
    len: usize,
}

impl Over {
    fn new(from: usize) -> Over {
        Over { from, lines: [Text::new(); MAX_LINES], len: 0 }
    }

    fn line(&mut self, args: fmt::Arguments) {
        let _ = self.lines[self.len].write_fmt(args);
        self.len += 1;
    }

    pub fn lines(&self) -> &[Text<MAX_TEXT>] {
        &self.lines[..self.len]
    }

    /// The last line of an exchange over, which gives the contact back.
    fn hand_back(&mut self, style: Style, ur: &str, my: &str) {
        match style.hand_back {
            0 => self.line(format_args!("HW CPY? {ur} DE {my} <KN>")),
            1 => self.line(format_args!("HW? <AR> {ur} DE {my} K")),
            _ => self.line(format_args!("HW? BK")),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fact {
    Call,
    Report,
    Name,
    Qth,
    Rig,
    Power,
    Antenna,
    Weather,
    Temp,
    YearsLicensed,
    Age,
    Job,
}

/// A question about the over just sent, and the answer as sent.
#[derive(Clone, Copy, Debug)]
pub struct Question {
    pub prompt: &'static str,
    pub answer: Text<16>,
}

pub struct Ragchew {
    pub stations: [Station; 2],
    /// GM, GA or GE: one time of day for the contact.
    greeting: &'static str,
    /// The current over, below `OVERS`.
    over: usize,
    rng: Rng,
}

impl Ragchew {
    pub fn new(seed: u32) -> Ragchew {
        let mut rng = Rng::new(seed);
        let mut stations = [Station::new(&mut rng), Station::new(&mut rng)];
        while stations[1].name == stations[0].name {
            stations[1].name = NAMES[rng.below(NAMES.len() as u32) as usize];
        }
        let greeting = GREETINGS[rng.below(GREETINGS.len() as u32) as usize];
        Ragchew { stations, greeting, over: 0, rng }
    }

    /// The station sending the current over.
    pub fn sender(&self) -> &Station {
        &self.stations[self.over % 2]
    }

    pub fn over(&self) -> Over {
        let (me, you) = (self.sender(), &self.stations[1 - self.over % 2]);
        let (my, ur) = (me.call.as_str(), you.call.as_str());
        let (style, greeting, name) = (me.style, self.greeting, you.name);
        let mut o = Over::new(self.over % 2);
        match self.over {
            0 if style.long_cq => o.line(format_args!("CQ CQ CQ DE {my} {my} K")),
            0 => o.line(format_args!("CQ CQ DE {my} {my} K")),
            1 => o.line(format_args!("{ur} DE {my} {my} K")),
            2 | 3 => {
                if self.over == 2 {
                    o.line(format_args!("{ur} DE {my} {greeting} ES TNX FER CALL"));
                } else {
                    o.line(format_args!("R R {ur} DE {my} {greeting} {name} TNX FER RPRT"));
                }
                o.line(format_args!("UR RST {} {} =", me.report, me.report));
                let label = if style.op { "OP" } else { "NAME" };
                let near = if style.near { "QTH NR" } else { "QTH" };
                // A DX country joins the line while it fits one copy group, without NR if need be.
                let qth = [phrase(&[near, me.city, me.country]), phrase(&["QTH", me.city, me.country])]
                    .into_iter()
                    .find(|qth| !me.country.is_empty() && qth.len() <= MAX_GROUP)
                    .unwrap_or_else(|| phrase(&[near, me.city, me.state]));
                if style.qth_before_name {
                    o.line(format_args!("{} =", qth.as_str()));
                }
                o.line(format_args!("{label} {} {} =", me.name, me.name));
                if !style.qth_before_name {
                    o.line(format_args!("{} =", qth.as_str()));
                }
                o.hand_back(style, ur, my);
            }
            4 | 5 => {
                if self.over == 4 {
                    o.line(format_args!("{ur} DE {my} OK {name} TNX FER RPRT"));
                } else {
                    o.line(format_args!("{ur} DE {my} R R FB {name}"));
                }
                let (rig, watts) = (me.rig, me.watts);
                match style.rig {
                    0 => o.line(format_args!("RIG {rig} {watts}W =")),
                    1 => o.line(format_args!("RIG HR {rig} {watts}W =")),
                    _ => o.line(format_args!("RIG {rig} PWR {watts}W =")),
                }
                match (me.height_ft, me.metric) {
                    (0, _) => o.line(format_args!("ANT {} =", me.antenna)),
                    (feet, true) => o.line(format_args!("ANT {} UP {}M =", me.antenna, (feet as u16 * 3 + 5) / 10)),
                    (feet, false) => o.line(format_args!("ANT {} UP {feet} FT =", me.antenna)),
                }
                let (wx, temp, unit) = (me.weather, me.temp, if me.metric { 'C' } else { 'F' });
                match style.wx {
                    0 => o.line(format_args!("WX {wx} {temp}{unit} =")),
                    1 => o.line(format_args!("WX {wx} ES {temp}{unit} =")),
                    _ => o.line(format_args!("WX {wx} TEMP {temp}{unit} =")),
                }
                o.hand_back(style, ur, my);
            }
            6 | 7 => {
                if self.over == 6 {
                    o.line(format_args!("{ur} DE {my} OK {name} SOLID CPY"));
                } else {
                    o.line(format_args!("{ur} DE {my} FB {name} TNX INFO"));
                }
                if style.ham == 0 {
                    o.line(format_args!("HAM FER {} YRS =", me.years_licensed));
                    o.line(format_args!("AGE {} =", me.age));
                } else {
                    o.line(format_args!("BEEN HAM {} YRS =", me.years_licensed));
                    o.line(format_args!("AGE HR {} =", me.age));
                }
                if me.age >= RETIRE_AGE {
                    o.line(format_args!("RETIRED {} =", me.job));
                } else {
                    o.line(format_args!("WORK AS {} =", me.job));
                }
                o.hand_back(style, ur, my);
            }
            8 => {
                match style.bye {
                    0 => (o.line(format_args!("OK {name} TNX FER QSO")), o.line(format_args!("HPE CUAGN"))),
                    1 => (o.line(format_args!("R TNX FER QSO {name}")), o.line(format_args!("73 ES GL"))),
                    _ => (o.line(format_args!("TNX FER FB QSO {name}")), o.line(format_args!("73 ES GUD DX"))),
                };
                o.line(format_args!("{ur} DE {my} TU <SK>"));
            }
            9 => {
                match style.bye {
                    0 => o.line(format_args!("TU {name} 73 CUL")),
                    1 => o.line(format_args!("R TU {name} 73 GL")),
                    _ => o.line(format_args!("73 ES GL {name}")),
                }
                o.line(format_args!("{ur} DE {my} TU <SK> E E"));
            }
            _ => o.line(format_args!("E E")),
        }
        o
    }

    /// A question about one fact in the current over; None for an over without facts.
    pub fn question(&mut self) -> Option<Question> {
        let facts: &[Fact] = match self.over {
            0 | 1 => &[Fact::Call],
            2 | 3 => &[Fact::Report, Fact::Name, Fact::Qth],
            4 | 5 => &[Fact::Rig, Fact::Power, Fact::Antenna, Fact::Weather, Fact::Temp],
            6 | 7 => &[Fact::YearsLicensed, Fact::Age, Fact::Job],
            _ => &[],
        };
        let &fact = facts.get(self.rng.below(facts.len() as u32) as usize)?;
        let me = self.sender();
        let mut answer = Text::new();
        let (prompt, _) = match fact {
            Fact::Call => ("SENDER'S CALL?", write!(answer, "{}", me.call.as_str())),
            Fact::Report => ("RST?", write!(answer, "{}", me.report)),
            Fact::Name => ("NAME?", write!(answer, "{}", me.name)),
            Fact::Qth => ("QTH CITY?", write!(answer, "{}", me.city)),
            Fact::Rig => ("RIG?", write!(answer, "{}", me.rig)),
            Fact::Power => ("POWER IN WATTS?", write!(answer, "{}", me.watts)),
            Fact::Antenna => ("ANTENNA?", write!(answer, "{}", me.antenna)),
            Fact::Weather => ("WX?", write!(answer, "{}", me.weather)),
            Fact::Temp => ("TEMPERATURE?", write!(answer, "{}", me.temp)),
            Fact::YearsLicensed => ("YEARS LICENSED?", write!(answer, "{}", me.years_licensed)),
            Fact::Age => ("AGE?", write!(answer, "{}", me.age)),
            Fact::Job => ("JOB?", write!(answer, "{}", me.job)),
        };
        Some(Question { prompt, answer })
    }

    /// Moves to the next over; after the last, two new stations start a new contact.
    pub fn next(&mut self) {
        self.over += 1;
        if self.over == OVERS {
            self.stations = [Station::new(&mut self.rng), Station::new(&mut self.rng)];
            self.over = 0;
        }
    }
}

/// One line of a contact's exchanges or sign-offs, for head copy: no calls and no closing `=`.
pub fn sentence(rng: &mut Rng) -> Text<MAX_GROUP> {
    let mut chat = Ragchew::new(rng.below(u32::MAX));
    chat.over = 2 + rng.below(DITS as u32 - 2) as usize;
    let over = chat.over();
    let lines = || over.lines().iter().filter(|line| !crate::text::has(line.as_str(), " DE "));
    let line = lines().nth(rng.below(lines().count() as u32) as usize).map_or("", |line| line.as_str());
    Text::format(format_args!("{}", crate::text::trim_end(line, " =")))
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::drill::{score_copy, squash};
    use crate::letters::Letters;
    use crate::qso::{CANADA, DX};

    /// The quiz marks an answer from `ui/ragchew.rs` `COPY_X`, 130 px, to the score: 10 cells.
    const QUIZ_CELLS: usize = 10;

    /// An answer fits the quiz once its spaces are gone.
    fn fits_scoring(answer: &str) -> bool {
        squash::<{ QUIZ_CELLS + 1 }>(answer).len() <= QUIZ_CELLS
    }

    #[test]
    fn every_line_fits_the_player_and_ends_whole() {
        let mut chat = Ragchew::new(7);
        for _ in 0..40 * OVERS {
            let over = chat.over();
            for line in over.lines() {
                assert!(!line.is_empty() && line.len() < MAX_TEXT, "{line:?}");
            }
            // BK hands the contact back, so it can only end an over.
            let (last, before) = over.lines().split_last().unwrap();
            let hands_back = |l: &Text<MAX_TEXT>| l.as_str().split(' ').any(|w| w == "BK" || w == "<BK>");
            assert!(!before.iter().any(hands_back), "{over:?}");
            let last = last.as_str();
            assert!(["K", "<KN>", "<SK>", "E E"].iter().any(|end| last.ends_with(end)), "{last}");
            chat.next();
        }
    }

    #[test]
    fn dx_sends_its_country_when_it_fits() {
        let (mut sent, mut left_out) = (0, 0);
        for seed in 0..2000 {
            let mut chat = Ragchew::new(seed);
            for over in 2..4 {
                chat.over = over;
                let me = *chat.sender();
                if me.country.is_empty() {
                    continue;
                }
                let over = chat.over();
                let qth = over.lines().iter().find(|l| l.as_str().starts_with("QTH ")).unwrap().as_str();
                let fits = std::format!("QTH {} {}", me.city, me.country).len() <= MAX_GROUP;
                assert_eq!(qth.ends_with(&std::format!(" {} =", me.country)), fits, "{qth}");
                if fits { sent += 1 } else { left_out += 1 }
            }
        }
        assert!(sent > 0 && left_out > 0, "{sent} {left_out}");
    }

    #[test]
    fn stations_take_turns_and_a_new_contact_follows() {
        let mut chat = Ragchew::new(3);
        let first = chat.stations[0].call;
        let senders: std::vec::Vec<usize> = (0..OVERS).map(|_| (chat.over().from, chat.next()).0).collect();
        assert!(senders.iter().enumerate().all(|(k, &from)| from == k % 2), "{senders:?}");
        assert!(chat.over().lines()[0].as_str().starts_with("CQ"));
        assert_ne!(chat.stations[0].call, first);
    }

    #[test]
    fn answers_are_in_the_over_and_score_as_copied() {
        let mut chat = Ragchew::new(11);
        let mut letters = Letters::default();
        for _ in 0..20 * OVERS {
            let over = chat.over();
            if let Some(q) = chat.question() {
                assert!(over.lines().iter().any(|l| l.as_str().contains(q.answer.as_str())), "{q:?}");
                assert!(fits_scoring(q.answer.as_str()), "{q:?}");
                let score = score_copy(q.answer.as_str(), &q.answer.as_str().to_ascii_lowercase(), &mut letters);
                assert_eq!(score.right as usize, score.len);
            }
            chat.next();
        }
    }

    #[test]
    fn every_city_fits_scoring() {
        for &(city, _) in CITIES_BY_AREA.iter().flat_map(|c| c.iter()) {
            assert!(fits_scoring(city), "{city}");
        }
        for city in CANADA.iter().chain(&DX).flat_map(|a| a.cities) {
            assert!(fits_scoring(city), "{city}");
        }
    }

    #[test]
    fn every_sentence_fits_a_group() {
        for seed in 0..5000 {
            let mut chat = Ragchew::new(seed);
            for over in 2..DITS {
                chat.over = over;
                for line in chat.over().lines().iter().filter(|line| !line.as_str().contains(" DE ")) {
                    assert!(line.as_str().trim_end_matches(" =").len() <= MAX_GROUP, "{}", line.as_str());
                }
            }
        }
        let mut rng = Rng::new(3);
        for _ in 0..500 {
            let line = sentence(&mut rng);
            assert!(!line.as_str().is_empty() && !line.as_str().ends_with('='), "{}", line.as_str());
        }
    }
}
