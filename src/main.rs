use anyhow::Result;
use anyhow::anyhow;
use clap::Parser;

use std::{
    fs::{self, Metadata},
    io, mem,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Time {
    duration_since_epoch: Duration,
    offset: i64,
}

impl From<SystemTime> for Time {
    fn from(value: SystemTime) -> Self {
        let duration_since_epoch = value
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(1));
        Self {
            duration_since_epoch,
            offset: Self::get_local_timezone_offset(duration_since_epoch.as_secs() as i64),
        }
    }
}

impl Time {
    pub fn from_created(metadata: &Metadata) -> io::Result<Self> {
        let created = metadata.created()?;
        Ok(Self::from(created))
    }
    pub fn from_modified(metadata: &Metadata) -> io::Result<Self> {
        let modified = metadata.modified()?;
        Ok(Self::from(modified))
    }
    fn get_local_timezone_offset(duration_since_epoch: i64) -> i64 {
        use libc::{localtime_r, time_t, tm};

        unsafe {
            let timestamp = duration_since_epoch as time_t;
            let mut tm_result: tm = mem::zeroed();

            if !localtime_r(&timestamp, &mut tm_result).is_null() {
                tm_result.tm_gmtoff
            } else {
                0
            }
        }
    }
    fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }
    fn get_days_in_year(year: i32) -> i32 {
        if Self::is_leap_year(year) { 366 } else { 365 }
    }
    fn get_days_in_month(month: u32, year: i32) -> i32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if Self::is_leap_year(year) {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        }
    }
    fn _get_day_of_week(&self) -> u32 {
        let (year, month, day) = self.to_calendar_date();
        let (m, y) = if month < 3 {
            (month + 12, year - 1)
        } else {
            (month, year)
        };

        let k = y % 100;
        let j = y / 100;
        let h = (day as i32 + (13 * (m as i32 + 1)) / 5 + k + k / 4 + j / 4 - 2 * j) % 7;
        ((h + 5) % 7) as u32
    }
    fn secs(&self) -> u64 {
        self.duration_since_epoch.as_secs()
    }
    fn to_calendar_date(&self) -> (i32, u32, u32) {
        let secs = self.secs() + self.offset as u64;
        let mut days = secs as i32 / 86400;
        // let rem_secs = secs % 86400;

        let mut year = 1970;
        let mut days_in_year = Self::get_days_in_year(year);

        while days >= days_in_year {
            days -= days_in_year;
            year += 1;
            days_in_year = Self::get_days_in_year(year);
        }

        while days < 0 {
            year -= 1;
            days_in_year = Self::get_days_in_year(year);
            days += days_in_year;
        }

        let mut month = 1;
        let mut days_in_month = Self::get_days_in_month(month, year);

        while days >= days_in_month {
            days -= days_in_month;
            month += 1;
            if month > 12 {
                month = 1;
                year += 1;
            }
            days_in_month = Self::get_days_in_month(month, year);
        }

        let day = (days + 1) as u32;

        (year, month, day)
    }
    fn to_time_parts(&self) -> (u32, u32, u32) {
        let secs = (self.secs() as u32 + self.offset as u32) % 86400;

        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;

        (hours, minutes, seconds)
    }
    pub fn format(&self) -> String {
        let (_year, month, day) = self.to_calendar_date();
        let (hours, minutes, _seconds) = self.to_time_parts();
        // let day_of_week = self.get_day_of_week();

        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let month_str = months.get((month - 1) as usize).unwrap_or(&"???");

        format!("{month_str} {day:>2} {hours:02}:{minutes:02}")
    }
}

fn terminal_size() -> Option<(u16, u16)> {
    use libc::{TIOCGWINSZ, ioctl};
    use std::io;
    use std::mem::MaybeUninit;
    use std::os::fd::AsRawFd;

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    let stdout = io::stdout();
    let fd_stdout = stdout.as_raw_fd();

    let mut size: MaybeUninit<Winsize> = MaybeUninit::uninit();

    unsafe {
        if ioctl(fd_stdout, TIOCGWINSZ, size.as_mut_ptr()) != -1 {
            let size = size.assume_init();
            Some((size.ws_col, size.ws_row))
        } else {
            None
        }
    }
}

#[derive(Debug, Parser)]
struct LssConf {
    #[clap(default_value = ".")]
    path: String,

    #[clap(short = 'H', long)]
    humanize: bool,

    #[clap(short, long)]
    all: bool,
    #[clap(short, long)]
    long: bool,

    #[clap(short = 'S', long = "size")]
    size_sort: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Color {
    Red,
    Aqua,
    Blue,
    Green,
    #[default]
    White,
}

impl Color {
    fn get_code(&self) -> &str {
        match self {
            Color::Red => "\x1b[31m",
            Color::Green => "\x1b[32m",
            Color::Blue => "\x1b[34m",
            Color::Aqua => "\x1b[36m",
            Color::White => "\x1b[37m",
        }
    }
    fn reset(&self) -> &str {
        "\x1b[0m"
    }
    fn wrap<S: AsRef<str>>(&self, s: S) -> String {
        format!("{}{}{}", self.get_code(), s.as_ref(), self.reset())
    }
}

#[derive(Debug, Default)]
struct Style {
    suffix: Option<char>,
    color: Color,
}

enum FType {
    File(bool),
    Dir,
    Symlink(String),
    BrokenSymlink,
    Other,
}

struct FEntry {
    name: String,
    path: PathBuf,
    ftype: FType,
    modified: Time,
    size: u64,
    hsize: String,
    owner: String,
    group: String,
    mode: String,
}

impl FEntry {
    fn get_styled_name(&self, suf: bool) -> String {
        let style = match self.ftype {
            FType::File(true) => Style {
                suffix: None,
                color: Color::Green,
            },
            FType::File(false) | FType::Other => Style {
                suffix: None,
                color: Color::White,
            },
            FType::Dir => Style {
                suffix: Some('/'),
                color: Color::Blue,
            },
            FType::Symlink(_) => Style {
                suffix: Some('@'),
                color: Color::Aqua,
            },
            FType::BrokenSymlink => Style {
                suffix: Some('!'),
                color: Color::Red,
            },
        };

        if suf && style.suffix.is_some() {
            format!("{}{}", style.color.wrap(&self.name), style.suffix.unwrap())
        } else {
            style.color.wrap(&self.name)
        }
    }
    fn to_fixed_str(&self, is_human: bool, maxs: &Maxs) -> String {
        let (size, len) = if is_human {
            (self.hsize.clone(), maxs.hsize)
        } else {
            (self.size.to_string(), maxs.size)
        };
        let name = if let FType::Symlink(target) = &self.ftype {
            format!("{} -> {}", self.get_styled_name(false), target)
        } else {
            self.get_styled_name(true)
        };
        format!(
            "{mode} {owner:>ownl$} {group:>grpl$} {size:>szl$} {modified} {name}",
            mode = self.mode,
            owner = self.owner,
            ownl = maxs.owner,
            group = self.group,
            grpl = maxs.group,
            size = size,
            szl = len,
            modified = self.modified.format(),
            name = name,
        )
    }
    fn to_str(&self) -> String {
        self.get_styled_name(true)
    }
}

fn get_human_readable_size(size: u64) -> String {
    let mut size = size as f64;
    let mut suffix = "B";
    if size > 1024. {
        size /= 1024.;
        suffix = "K";
    }
    if size > 1024. {
        size /= 1024.;
        suffix = "M";
    }
    if size > 1024. {
        size /= 1024.;
        suffix = "G";
    }

    let rounded = (size * 100.).round() / 100.;

    if rounded.fract() == 0. {
        format!("{}{}", rounded as i64, suffix)
    } else if rounded.fract() * 10. % 1. == 0. {
        format!("{rounded:.1}{suffix}")
    } else {
        format!("{rounded:.2}{suffix}")
    }
}

fn get_username(uid: u32) -> Result<String> {
    let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0u8; 1024];
    let mut res: *mut libc::passwd = std::ptr::null_mut();

    let ret = unsafe {
        libc::getpwuid_r(
            uid,
            &mut passwd,
            buf.as_mut_ptr() as *mut _,
            buf.len(),
            &mut res,
        )
    };

    if ret == 0 && !res.is_null() {
        Ok(unsafe { std::ffi::CStr::from_ptr(passwd.pw_name) }
            .to_str()
            .map(|s| s.to_string())?)
    } else {
        Err(anyhow!("get username error"))
    }
}

fn get_groupname(gid: u32) -> Result<String> {
    let mut group: libc::group = unsafe { std::mem::zeroed() };
    let mut buf = vec![0u8; 1024];
    let mut res: *mut libc::group = std::ptr::null_mut();

    let ret = unsafe {
        libc::getgrgid_r(
            gid,
            &mut group,
            buf.as_mut_ptr() as *mut _,
            buf.len(),
            &mut res,
        )
    };

    if ret == 0 && !res.is_null() {
        Ok(unsafe { std::ffi::CStr::from_ptr(group.gr_name) }
            .to_str()
            .map(|s| s.to_owned())?)
    } else {
        Err(anyhow!("get groupname error"))
    }
}

fn get_owner_and_group(md: &Metadata) -> Result<(String, String)> {
    Ok((get_username(md.uid())?, get_groupname(md.gid())?))
}

fn get_mode(md: &Metadata) -> String {
    let perm = md.permissions();
    let mode = perm.mode();

    let mut builder = String::with_capacity(10);

    let ft = md.file_type();
    builder.push(if ft.is_dir() {
        'd'
    } else if ft.is_file() {
        '-'
    } else if ft.is_symlink() {
        'l'
    } else {
        '?'
    });

    // User permissions
    builder.push(if mode & 0o400 != 0 { 'r' } else { '-' });
    builder.push(if mode & 0o200 != 0 { 'w' } else { '-' });
    builder.push(if mode & 0o100 != 0 {
        if mode & 0o4000 != 0 { 's' } else { 'x' }
    } else {
        if mode & 0o4000 != 0 { 'S' } else { '-' }
    });

    // Group permissions
    builder.push(if mode & 0o040 != 0 { 'r' } else { '-' });
    builder.push(if mode & 0o020 != 0 { 'w' } else { '-' });
    builder.push(if mode & 0o010 != 0 {
        if mode & 0o2000 != 0 { 's' } else { 'x' }
    } else {
        if mode & 0o2000 != 0 { 'S' } else { '-' }
    });

    // Other permissions
    builder.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    builder.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    builder.push(if mode & 0o001 != 0 {
        if mode & 0o1000 != 0 { 't' } else { 'x' }
    } else {
        if mode & 0o1000 != 0 { 'T' } else { '-' }
    });

    builder
}

#[derive(Debug, Default)]
struct Maxs {
    size: usize,
    hsize: usize,
    name: usize,
    owner: usize,
    group: usize,
}

fn read_dir<P: AsRef<Path>>(path: P) -> Result<(Vec<FEntry>, Maxs)> {
    let mut res = Vec::new();

    let mut maxs = Maxs::default();

    for f in fs::read_dir(path)? {
        let f = f?;
        let md = f.metadata()?;

        let name = f
            .file_name()
            .to_str()
            .ok_or(anyhow!("non-valid unicode in name"))?
            .to_string();

        if name.len() > maxs.name {
            maxs.name = name.len();
        }

        let ftype = if md.is_dir() {
            FType::Dir
        } else if md.is_symlink() {
            match fs::read_link(f.path()) {
                Ok(p) => FType::Symlink(
                    p.to_str()
                        .ok_or(anyhow!("non-valid unicode in name"))?
                        .to_string(),
                ),
                Err(_) => FType::BrokenSymlink,
            }
        } else if md.is_file() {
            FType::File(md.is_file() && md.permissions().mode() & 0o111 != 0)
        } else {
            FType::Other
        };

        let modified = Time::from_modified(&md)?;

        let size = md.size();
        if size.to_string().len() > maxs.size {
            maxs.size = size.to_string().len();
        }
        let hsize = get_human_readable_size(size);
        if hsize.len() > maxs.hsize {
            maxs.hsize = hsize.len();
        }

        let (owner, group) = get_owner_and_group(&md)?;
        if owner.len() > maxs.owner {
            maxs.owner = owner.len();
        }
        if group.len() > maxs.group {
            maxs.group = group.len();
        }

        let mode = get_mode(&md);

        res.push(FEntry {
            name,
            path: f.path(),
            ftype,
            modified,
            size,
            hsize,
            owner,
            group,
            mode,
        })
    }

    Ok((res, maxs))
}

fn format_long_info(names: Vec<String>) -> String {
    if names.is_empty() {
        return String::new();
    }

    names.join("\n")
}
fn format_with_terminal_width(names: Vec<String>) -> String {
    if names.is_empty() {
        return String::new();
    }

    let (term_cols, _) = terminal_size().unwrap_or((80, 24));
    let term_cols = term_cols as usize;

    let total_width = names.iter().map(|n| n.len()).sum::<usize>() + names.len() - 1;
    if total_width <= term_cols {
        return names.join(" ");
    }

    let max_width = names.iter().map(|n| n.len()).max().unwrap_or(1);
    let col_width = max_width + 2;
    let max_cols = (term_cols / col_width).max(1);
    let total_items = names.len();
    let rows = total_items.div_ceil(max_cols);

    let mut output = String::new();
    for row in 0..rows {
        let mut line = String::new();

        for col in 0..max_cols {
            let idx = col * rows + row;
            if idx < total_items {
                let name = &names[idx];
                line.push_str(&format!("{name:<col_width$}"));
            }
        }

        output.push_str(line.trim_end());
        output.push('\n');
    }

    output.trim_end().to_string()
}

fn main() -> Result<()> {
    let conf = LssConf::parse();
    let (mut dir, maxs) = read_dir(&conf.path)?;
    dir.retain(|f| !f.name.starts_with(".") || conf.all);
    if conf.size_sort {
        dir.sort_by_key(|k| k.size);
    } else {
        dir.sort_by_key(|k| k.name.clone());
    }

    if conf.long {
        let mut names = dir
            .iter()
            .map(|f| f.to_fixed_str(conf.humanize, &maxs))
            .collect();
        println!("{}", format_long_info(names));
    } else {
        let names = dir.iter().map(|f| f.to_str()).collect();
        print!("{} ", format_with_terminal_width(names));
        println!();
    }
    Ok(())
}
