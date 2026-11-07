//! CMOS real-time clock (RTC) reader using the MC146818-compatible ports.

use core::arch::asm;

const CMOS_INDEX: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;
const REG_A: u8 = 0x0A;
const REG_B: u8 = 0x0B;
const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
const REG_DAY: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
const REG_CENTURY: u8 = 0x32;
const UPDATE_IN_PROGRESS: u8 = 0x80;
const BINARY_MODE: u8 = 0x04;
const TWENTY_FOUR_HOUR: u8 = 0x02;
const PM: u8 = 0x80;
const MAX_UIP_POLLS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Reads a consistent calendar snapshot from the CMOS RTC.
///
/// Returns None if the clock remains in an update cycle or contains invalid
/// calendar/time fields. The NMI-disable bit is preserved in each index write
/// only for the duration of the register access; NMI is re-enabled afterwards.
pub fn read() -> Option<DateTime> {
    for _ in 0..3 {
        if !wait_for_update_end() {
            return None;
        }

        let status_b = read_register(REG_B);
        let first = read_raw();
        if !wait_for_update_end() {
            return None;
        }
        let second = read_raw();

        if first == second {
            return decode(second, status_b);
        }
    }
    None
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct RawTime {
    seconds: u8,
    minutes: u8,
    hours: u8,
    day: u8,
    month: u8,
    year: u8,
    century: u8,
}

fn read_raw() -> RawTime {
    RawTime {
        seconds: read_register(REG_SECONDS),
        minutes: read_register(REG_MINUTES),
        hours: read_register(REG_HOURS),
        day: read_register(REG_DAY),
        month: read_register(REG_MONTH),
        year: read_register(REG_YEAR),
        century: read_register(REG_CENTURY),
    }
}

fn wait_for_update_end() -> bool {
    for _ in 0..MAX_UIP_POLLS {
        if read_register(REG_A) & UPDATE_IN_PROGRESS == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn decode(raw: RawTime, status_b: u8) -> Option<DateTime> {
    let binary = status_b & BINARY_MODE != 0;
    let hour_24 = status_b & TWENTY_FOUR_HOUR != 0;
    let pm = raw.hours & PM != 0;
    let raw_hour = raw.hours & !PM;

    let convert = |value: u8| if binary { Some(value) } else { from_bcd(value) };
    let seconds = convert(raw.seconds)?;
    let minutes = convert(raw.minutes)?;
    let mut hours = convert(raw_hour)?;
    let day = convert(raw.day)?;
    let month = convert(raw.month)?;
    let year = convert(raw.year)?;
    let century = convert(raw.century).filter(|value| (19..=99).contains(value));

    if !hour_24 {
        if !(1..=12).contains(&hours) {
            return None;
        }
        hours %= 12;
        if pm {
            hours += 12;
        }
    }

    let full_year = century
        .map(|century| u16::from(century) * 100 + u16::from(year))
        .unwrap_or_else(|| {
            if year >= 70 {
                1900 + u16::from(year)
            } else {
                2000 + u16::from(year)
            }
        });

    if seconds > 59
        || minutes > 59
        || hours > 23
        || month == 0
        || month > 12
        || day == 0
        || day > days_in_month(full_year, month)
    {
        return None;
    }

    Some(DateTime {
        year: full_year,
        month,
        day,
        hour: hours,
        minute: minutes,
        second: seconds,
    })
}

fn from_bcd(value: u8) -> Option<u8> {
    let high = value >> 4;
    let low = value & 0x0F;
    if high > 9 || low > 9 {
        return None;
    }
    Some(high * 10 + low)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn read_register(register: u8) -> u8 {
    // SAFETY: 0x70/0x71 are the standard CMOS index/data ports. The index is
    // written with NMI enabled (bit 7 clear), so normal NMI delivery continues.
    unsafe {
        outb(CMOS_INDEX, register & 0x7F);
        inb(CMOS_DATA)
    }
}

unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{decode, DateTime, RawTime, BINARY_MODE, TWENTY_FOUR_HOUR};

    fn raw(
        seconds: u8,
        minutes: u8,
        hours: u8,
        day: u8,
        month: u8,
        year: u8,
        century: u8,
    ) -> RawTime {
        RawTime {
            seconds,
            minutes,
            hours,
            day,
            month,
            year,
            century,
        }
    }

    fn expected(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> DateTime {
        DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    #[test]
    fn decodes_bcd_timestamp_with_century() {
        let result = decode(
            raw(0x56, 0x34, 0x21, 0x09, 0x11, 0x26, 0x20),
            TWENTY_FOUR_HOUR,
        );
        assert_eq!(result, Some(expected(2026, 11, 9, 21, 34, 56)));
    }

    #[test]
    fn decodes_binary_timestamp_in_24_hour_mode() {
        let status = BINARY_MODE | TWENTY_FOUR_HOUR;
        let result = decode(raw(58, 47, 23, 28, 2, 24, 20), status);
        assert_eq!(result, Some(expected(2024, 2, 28, 23, 47, 58)));
    }

    #[test]
    fn converts_12_am_to_midnight() {
        let result = decode(raw(0x00, 0x05, 0x12, 0x01, 0x01, 0x26, 0x20), 0);
        assert_eq!(result, Some(expected(2026, 1, 1, 0, 5, 0)));
    }

    #[test]
    fn converts_12_pm_to_noon() {
        let result = decode(raw(0x00, 0x05, 0x92, 0x01, 0x01, 0x26, 0x20), 0);
        assert_eq!(result, Some(expected(2026, 1, 1, 12, 5, 0)));
    }

    #[test]
    fn validates_leap_day_using_gregorian_rules() {
        let leap_day = decode(raw(0, 0, 0x92, 0x29, 0x02, 0x00, 0x20), 0);
        let non_leap_century = decode(raw(0, 0, 0x92, 0x29, 0x02, 0x00, 0x19), 0);
        assert_eq!(leap_day, Some(expected(2000, 2, 29, 12, 0, 0)));
        assert_eq!(non_leap_century, None);
    }

    #[test]
    fn rejects_invalid_bcd_nibbles() {
        let invalid_seconds = decode(raw(0x6A, 0x00, 0x12, 0x01, 0x01, 0x26, 0x20), 0);
        assert_eq!(invalid_seconds, None);
    }

    #[test]
    fn rejects_out_of_range_calendar_and_time_fields() {
        let invalid_seconds = decode(raw(0x60, 0, 0x12, 0x01, 0x01, 0x26, 0x20), 0);
        let invalid_minutes = decode(raw(0, 0x60, 0x12, 0x01, 0x01, 0x26, 0x20), 0);
        let invalid_month = decode(raw(0, 0, 0x12, 0x01, 0x13, 0x26, 0x20), 0);
        let invalid_day = decode(raw(0, 0, 0x12, 0x00, 0x01, 0x26, 0x20), 0);
        let invalid_12_hour_clock = decode(raw(0, 0, 0x00, 0x01, 0x01, 0x26, 0x20), 0);
        assert_eq!(invalid_seconds, None);
        assert_eq!(invalid_minutes, None);
        assert_eq!(invalid_month, None);
        assert_eq!(invalid_day, None);
        assert_eq!(invalid_12_hour_clock, None);
    }

    #[test]
    fn falls_back_to_pivot_year_when_century_is_unavailable_or_invalid() {
        let no_century_2069 = decode(raw(0, 0, 0x92, 0x01, 0x01, 0x69, 0x00), 0);
        let no_century_1970 = decode(raw(0, 0, 0x92, 0x01, 0x01, 0x70, 0x00), 0);
        let invalid_bcd_century = decode(raw(0, 0, 0x92, 0x01, 0x01, 0x26, 0xFA), 0);
        let implausible_century = decode(raw(0, 0, 0x92, 0x01, 0x01, 0x26, 0x18), 0);
        assert_eq!(no_century_2069, Some(expected(2069, 1, 1, 12, 0, 0)));
        assert_eq!(no_century_1970, Some(expected(1970, 1, 1, 12, 0, 0)));
        assert_eq!(invalid_bcd_century, Some(expected(2026, 1, 1, 12, 0, 0)));
        assert_eq!(implausible_century, Some(expected(2026, 1, 1, 12, 0, 0)));
    }
}
