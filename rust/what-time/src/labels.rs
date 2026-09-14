//! Numeric model roles. The inference kernel and the compiler share these ids.

/// The 35 named semantic roles the classifier emits, in id order.
pub const LABELS: [&str; 35] = [
    "O",
    "NUM",
    "ORD",
    "UNIT",
    "DIR_BEFORE",
    "DIR_AFTER",
    "NOW",
    "REL_DAY",
    "DEICTIC",
    "WEEKDAY",
    "DAYGROUP",
    "MONTH",
    "DOM",
    "YEAR",
    "HOUR",
    "MINUTE",
    "SECOND",
    "MERIDIEM",
    "TIME_NAMED",
    "DAYPART",
    "RANGE_START",
    "RANGE_END",
    "RECUR",
    "FREQ",
    "TIMES",
    "BOUND_START",
    "BOUND_END",
    "COUNT",
    "DUR",
    "EXCEPT",
    "HOLIDAY",
    "JOIN",
    "GLUE",
    "EDGE",
    "CLOCK_OFFSET",
];

/// Total output slots: 35 named roles plus 5 reserved.
#[allow(dead_code)]
pub const LABEL_COUNT: u8 = 40;

/// Numeric roles stay internal; source-facing diagnostics use [`LABELS`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Role {
    O = 0,
    Num = 1,
    Ord = 2,
    Unit = 3,
    DirBefore = 4,
    DirAfter = 5,
    Now = 6,
    RelDay = 7,
    Deictic = 8,
    Weekday = 9,
    DayGroup = 10,
    Month = 11,
    Dom = 12,
    Year = 13,
    Hour = 14,
    Minute = 15,
    Second = 16,
    Meridiem = 17,
    TimeNamed = 18,
    DayPart = 19,
    RangeStart = 20,
    RangeEnd = 21,
    Recur = 22,
    Freq = 23,
    Times = 24,
    BoundStart = 25,
    BoundEnd = 26,
    Count = 27,
    Dur = 28,
    Except = 29,
    Holiday = 30,
    Join = 31,
    Glue = 32,
    Edge = 33,
    ClockOffset = 34,
}

impl Role {
    const ALL: [Role; 35] = [
        Role::O,
        Role::Num,
        Role::Ord,
        Role::Unit,
        Role::DirBefore,
        Role::DirAfter,
        Role::Now,
        Role::RelDay,
        Role::Deictic,
        Role::Weekday,
        Role::DayGroup,
        Role::Month,
        Role::Dom,
        Role::Year,
        Role::Hour,
        Role::Minute,
        Role::Second,
        Role::Meridiem,
        Role::TimeNamed,
        Role::DayPart,
        Role::RangeStart,
        Role::RangeEnd,
        Role::Recur,
        Role::Freq,
        Role::Times,
        Role::BoundStart,
        Role::BoundEnd,
        Role::Count,
        Role::Dur,
        Role::Except,
        Role::Holiday,
        Role::Join,
        Role::Glue,
        Role::Edge,
        Role::ClockOffset,
    ];

    pub fn from_u8(value: u8) -> Option<Role> {
        Self::ALL.get(value as usize).copied()
    }

    pub fn name(self) -> &'static str {
        LABELS[self as usize]
    }

    pub fn from_name(name: &str) -> Option<Role> {
        LABELS
            .iter()
            .position(|label| *label == name)
            .and_then(|index| Role::from_u8(index as u8))
    }
}
