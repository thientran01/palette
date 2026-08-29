//! Palette Surfaces MIRROR spike — live-clone an already-open window via DWM
//! thumbnails. Standing alone off the 1.0 path; do not fold this into Palette.
//!
//! The question: can `DwmRegisterThumbnail` clone a YouTube Chrome/Edge/Brave
//! tab into a small always-on-top tao window, well enough to watch over a
//! borderless-fullscreen game? Opaque dest first (the reliable path).
//! Transparent/layered is a documented second experiment (`--layered`), not
//! the ship path. No input forwarding — a click raises the source window.
//!
//! Usage:
//!   mirror-spike                  pick / auto YouTube
//!   mirror-spike list             print visible top-level windows and exit
//!   mirror-spike youtube          first YouTube-looking browser window
//!   mirror-spike 3                pick list index
//!   mirror-spike "burning"        title substring
//!   mirror-spike --crop youtube   also inset typical Chromium tab+toolbar
//!   mirror-spike --layered        WS_EX_LAYERED dest (does the thumb draw?)
//!
//! Seat lives in %APPDATA%\palette-mirror-spike\seat.txt.
//! Deliverable: docs/mirror-spike-matrix.md — Windows-live cells are for Thien.
//!
//! Console stays attached (no `windows_subsystem = "windows"`) — the list,
//! HRESULTs, and minimize/raise lines are the measurement.

use std::env;

/// Chromium tab strip + address bar at 96 DPI. `--crop` scales this by the
/// source window's DPI. Guess, not a video-element finder.
#[cfg(windows)]
const CHROME_TOP_DIP: i32 = 88;

#[cfg(windows)]
const DEFAULT_W: f64 = 560.0;
#[cfg(windows)]
const DEFAULT_H: f64 = 315.0;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Picker {
    Auto,
    List,
    Youtube,
    Index(usize),
    Substring(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Opts {
    picker: Picker,
    layered: bool,
    crop: bool,
    help: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            picker: Picker::Auto,
            layered: false,
            crop: false,
            help: false,
        }
    }
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut opts = Opts::default();
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => opts.help = true,
            "--layered" | "-L" => opts.layered = true,
            "--crop" | "-c" => opts.crop = true,
            "list" | "--list" => opts.picker = Picker::List,
            "youtube" => opts.picker = Picker::Youtube,
            other if other.starts_with('-') => {
                return Err(format!("unknown flag: {other}"));
            }
            other => {
                if let Ok(i) = other.parse::<usize>() {
                    opts.picker = Picker::Index(i);
                } else {
                    opts.picker = Picker::Substring(other.to_string());
                }
            }
        }
    }
    Ok(opts)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_browser(exe: &str, class: &str) -> bool {
    let exe = exe.to_ascii_lowercase();
    let class_l = class.to_ascii_lowercase();
    matches!(
        exe.as_str(),
        "chrome.exe"
            | "msedge.exe"
            | "brave.exe"
            | "firefox.exe"
            | "opera.exe"
            | "vivaldi.exe"
            | "chromium.exe"
    ) || class.starts_with("Chrome_WidgetWin")
        || class_l == "mozillawindowclass"
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_youtube_title(title: &str) -> bool {
    title.to_ascii_lowercase().contains("youtube")
}

fn usage() {
    eprintln!(
        "\
mirror-spike — live-clone a window via DwmRegisterThumbnail (Windows only).

  cargo run                  pick / auto YouTube
  cargo run -- list          enumerate visible top-level windows
  cargo run -- youtube       first YouTube-looking browser window
  cargo run -- 3             pick that list index
  cargo run -- \"burning\"     title substring (topmost match)

  --crop                     cheap skip of typical Chromium tab+toolbar
  --layered                  second experiment: WS_EX_LAYERED dest
  --help                     this text

Opaque dest is the path under test. Click the mirror to raise the source.
No input forwarding. Seat: %APPDATA%\\palette-mirror-spike\\seat.txt
See docs/mirror-spike-matrix.md for the questions + live checkboxes."
    );
}

#[cfg(not(windows))]
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_opts(&args) {
        Ok(opts) if opts.help => {
            usage();
        }
        Ok(_) => {
            usage();
            eprintln!();
            eprintln!("This crate is a Windows DWM probe (MSVC). The Linux agent can cargo check;");
            eprintln!("the live cells in docs/mirror-spike-matrix.md are for a Windows run.");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("mirror-spike: {e}");
            usage();
            std::process::exit(2);
        }
    }
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let opts = match parse_opts(&args) {
        Ok(o) if o.help => {
            usage();
            return;
        }
        Ok(o) => o,
        Err(e) => {
            eprintln!("mirror-spike: {e}");
            usage();
            std::process::exit(2);
        }
    };
    if let Err(e) = win::run(opts) {
        eprintln!("mirror-spike: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::ffi::c_void;
    use std::fs;
    use std::io::{self, IsTerminal, Write};
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;

    use tao::{
        dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
        event::{ElementState, Event, MouseButton, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
        platform::windows::WindowExtWindows,
        window::WindowBuilder,
    };
    use windows::core::{Error as WinError, BOOL, PWSTR};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail,
        DwmUnregisterThumbnail, DwmUpdateThumbnailProperties, DWMWA_CLOAKED,
        DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE,
        DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
    };
    use windows::Win32::Graphics::Gdi::{
        CreateSolidBrush, DeleteObject, DrawTextW, FillRect, GetDC, ReleaseDC, SetBkMode,
        SetTextColor, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HGDIOBJ, TRANSPARENT,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetClassNameW, GetClientRect, GetForegroundWindow,
        GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
        IsWindow, IsWindowVisible, SetForegroundWindow, SetLayeredWindowAttributes,
        SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, LWA_ALPHA, SW_RESTORE, WS_EX_LAYERED,
        WS_EX_TOOLWINDOW,
    };

    #[derive(Clone)]
    pub struct Entry {
        pub hwnd: HWND,
        pub title: String,
        pub class: String,
        pub exe: String,
        pub pid: u32,
        pub iconic: bool,
        pub youtube: bool,
        pub browser: bool,
    }

    fn data_dir() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("palette-mirror-spike")
    }

    fn seat_path() -> PathBuf {
        data_dir().join("seat.txt")
    }

    fn load_seat() -> Option<(i32, i32, u32, u32)> {
        let s = fs::read_to_string(seat_path()).ok()?;
        let mut it = s.split_whitespace().filter_map(|t| t.parse::<i64>().ok());
        let (x, y) = (it.next()?, it.next()?);
        let (w, h) = (it.next()?, it.next()?);
        if w < 200 || h < 120 {
            return None;
        }
        Some((x as i32, y as i32, w as u32, h as u32))
    }

    fn save_seat(pos: PhysicalPosition<i32>, size: PhysicalSize<u32>) {
        let _ = fs::create_dir_all(data_dir());
        let _ = fs::write(
            seat_path(),
            format!("{} {} {} {}", pos.x, pos.y, size.width, size.height),
        );
    }

    fn exe_name(pid: u32) -> String {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return format!("<pid {pid}>");
            };
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let name = if QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok()
            {
                let full = String::from_utf16_lossy(&buf[..len as usize]);
                full.rsplit('\\').next().unwrap_or(&full).to_string()
            } else {
                format!("<pid {pid}>")
            };
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            name
        }
    }

    fn utf16_text(hwnd: HWND, f: unsafe fn(HWND, &mut [u16]) -> i32) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe { f(hwnd, &mut buf) };
        if n <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buf[..n as usize])
        }
    }

    fn is_cloaked(hwnd: HWND) -> bool {
        let mut cloaked: u32 = 0;
        unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut c_void,
                std::mem::size_of::<u32>() as u32,
            )
            .ok()
            .map(|_| cloaked != 0)
            .unwrap_or(false)
        }
    }

    fn is_tool_window(hwnd: HWND) -> bool {
        let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        ex & WS_EX_TOOLWINDOW.0 != 0
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<Entry>) };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return true.into();
            }
            if is_tool_window(hwnd) || is_cloaked(hwnd) {
                return true.into();
            }
            let title = utf16_text(hwnd, GetWindowTextW);
            if title.trim().is_empty() {
                return true.into();
            }
            let class = utf16_text(hwnd, GetClassNameW);
            if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
                return true.into();
            }
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            if rect.right <= rect.left || rect.bottom <= rect.top {
                return true.into();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let exe = exe_name(pid);
            let browser = is_browser(&exe, &class);
            list.push(Entry {
                hwnd,
                youtube: browser && is_youtube_title(&title),
                browser,
                title,
                class,
                exe,
                pid,
                iconic: IsIconic(hwnd).as_bool(),
            });
        }
        true.into()
    }

    fn enumerate() -> Vec<Entry> {
        let mut list = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(enum_proc), LPARAM(&mut list as *mut _ as isize));
        }
        list
    }

    fn hwnd_hex(hwnd: HWND) -> String {
        format!("{:#x}", hwnd.0 as isize)
    }

    fn print_list(list: &[Entry]) {
        println!("#  hwnd        pid      exe                  class                    title");
        for (i, e) in list.iter().enumerate() {
            let mark = if e.youtube {
                " [youtube]"
            } else if e.browser {
                " [browser]"
            } else {
                ""
            };
            let frozen = if e.iconic { " [minimized]" } else { "" };
            println!(
                "{i:>2} {}  {:>7}  {:<20} {:<24} \"{}\"{mark}{frozen}",
                hwnd_hex(e.hwnd),
                e.pid,
                e.exe.chars().take(20).collect::<String>(),
                e.class.chars().take(24).collect::<String>(),
                e.title.chars().take(72).collect::<String>(),
            );
        }
    }

    fn pick(list: &[Entry], picker: &Picker) -> Result<usize, String> {
        if list.is_empty() {
            return Err("no visible top-level windows".into());
        }
        let youtube: Vec<usize> = list
            .iter()
            .enumerate()
            .filter(|(_, e)| e.youtube)
            .map(|(i, _)| i)
            .collect();

        match picker {
            Picker::List => unreachable!("list exits before pick"),
            Picker::Index(i) => {
                if *i >= list.len() {
                    return Err(format!("index {i} out of range (0..{})", list.len()));
                }
                Ok(*i)
            }
            Picker::Youtube => youtube.first().copied().ok_or_else(|| {
                "no YouTube-looking browser window (title contains \"youtube\")".into()
            }),
            Picker::Substring(s) => {
                let needle = s.to_ascii_lowercase();
                list.iter()
                    .position(|e| e.title.to_ascii_lowercase().contains(&needle))
                    .ok_or_else(|| format!("no window title contains {s:?}"))
            }
            Picker::Auto => {
                if youtube.len() == 1 {
                    Ok(youtube[0])
                } else if youtube.is_empty() {
                    prompt_index(list, "no YouTube window — enter an index")
                } else {
                    println!(
                        "{} YouTube windows; enter an index (or re-run with a substring).",
                        youtube.len()
                    );
                    prompt_index(list, "index")
                }
            }
        }
    }

    fn prompt_index(list: &[Entry], prompt: &str) -> Result<usize, String> {
        if !io::stdin().is_terminal() {
            return Err("ambiguous source and stdin is not a TTY — pass list / youtube / an index / a substring".into());
        }
        print!("{prompt}: ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let t = line.trim();
        let i: usize = t.parse().map_err(|_| format!("not an index: {t:?}"))?;
        if i >= list.len() {
            return Err(format!("index {i} out of range (0..{})", list.len()));
        }
        Ok(i)
    }

    fn dest_hwnd(window: &tao::window::Window) -> HWND {
        HWND(window.hwnd() as _)
    }

    fn apply_layered(hwnd: HWND) -> Result<u32, String> {
        unsafe {
            let before = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (before | WS_EX_LAYERED.0) as isize);
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 230, LWA_ALPHA)
                .map_err(|e| format!("SetLayeredWindowAttributes: {e}"))?;
            let after = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            println!(
                "layered experiment: exstyle {before:#x} → {after:#x}  WS_EX_LAYERED={}  alpha=230",
                after & WS_EX_LAYERED.0 != 0
            );
            println!("  watch the dest: does the thumbnail draw, go blank, or tint?");
            Ok(after)
        }
    }

    fn client_rect(hwnd: HWND) -> RECT {
        let mut r = RECT::default();
        let _ = unsafe { GetClientRect(hwnd, &mut r) };
        r
    }

    fn source_crop(thumb: isize, src: HWND, crop: bool) -> Option<RECT> {
        if !crop {
            return None;
        }
        let size = unsafe { DwmQueryThumbnailSourceSize(thumb).ok()? };
        let dpi = unsafe { GetDpiForWindow(src) }.max(96);
        let top = (CHROME_TOP_DIP as u32 * dpi / 96) as i32;
        if size.cy <= top + 40 {
            return None;
        }
        Some(RECT {
            left: 0,
            top,
            right: size.cx,
            bottom: size.cy,
        })
    }

    fn update_thumb(
        thumb: isize,
        dest: HWND,
        src: HWND,
        crop: bool,
        visible: bool,
    ) -> Result<(), WinError> {
        let dest_r = client_rect(dest);
        let src_r = source_crop(thumb, src, crop);
        let mut props = DWM_THUMBNAIL_PROPERTIES::default();
        props.dwFlags = DWM_TNP_RECTDESTINATION
            | DWM_TNP_VISIBLE
            | DWM_TNP_OPACITY
            | DWM_TNP_SOURCECLIENTAREAONLY;
        if let Some(src_r) = src_r {
            props.dwFlags |= DWM_TNP_RECTSOURCE;
            props.rcSource = src_r;
        }
        props.rcDestination = dest_r;
        props.opacity = 255;
        props.fVisible = BOOL::from(visible);
        // Client area only = skip the Win32 frame (title bar / borders). Still
        // the full browser page — not a video-element crop.
        props.fSourceClientAreaOnly = BOOL::from(true);
        unsafe { DwmUpdateThumbnailProperties(thumb, &props) }
    }

    fn paint_empty(dest: HWND, msg: &str) {
        unsafe {
            let dc = GetDC(Some(dest));
            if dc.0.is_null() {
                return;
            }
            let brush = CreateSolidBrush(COLORREF(0x00181010));
            let mut r = client_rect(dest);
            let _ = FillRect(dc, &r, brush);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0x00CCCCCC));
            let mut wide: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
            DrawTextW(
                dc,
                &mut wide,
                &mut r,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            let _ = ReleaseDC(Some(dest), dc);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
    }

    fn raise_source(src: HWND) {
        unsafe {
            // View-only: restore + foreground the real window. No synthesized
            // input into its client area (no click-through, no CDP).
            if IsIconic(src).as_bool() {
                let _ = ShowWindow(src, SW_RESTORE);
            }
            // We just received a click, so we own the foreground lock.
            let _ = SetForegroundWindow(src);
            let _ = BringWindowToTop(src);
        }
    }

    fn title_for(src: &Entry, frozen: bool, dead: bool) -> String {
        if dead {
            "mirror — source closed".into()
        } else if frozen {
            "mirror — frozen (source minimized)".into()
        } else {
            let t: String = src.title.chars().take(48).collect();
            format!("mirror — {t}")
        }
    }

    pub fn run(opts: Opts) -> Result<(), String> {
        let list = enumerate();
        print_list(&list);
        if matches!(opts.picker, Picker::List) {
            return Ok(());
        }
        println!();
        let idx = pick(&list, &opts.picker)?;
        let src = list[idx].clone();
        println!(
            "source [{idx}] {} {} \"{}\"",
            src.exe,
            hwnd_hex(src.hwnd),
            src.title
        );
        if src.iconic {
            println!(
                "  source is minimized — DWM will show a frozen last frame (OnTopReplica #71)."
            );
            println!("  restore it, or click the mirror to raise it.");
        }

        let event_loop = EventLoopBuilder::<()>::with_user_event().build();
        let proxy = event_loop.create_proxy();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(400));
            if proxy.send_event(()).is_err() {
                break;
            }
        });

        let mut builder = WindowBuilder::new()
            .with_title(title_for(&src, src.iconic, false))
            .with_always_on_top(true)
            .with_transparent(opts.layered);
        builder = match load_seat() {
            Some((x, y, w, h)) => builder
                .with_position(PhysicalPosition::new(x, y))
                .with_inner_size(PhysicalSize::new(w, h)),
            None => builder.with_inner_size(LogicalSize::new(DEFAULT_W, DEFAULT_H)),
        };
        let window = builder.build(&event_loop).map_err(|e| e.to_string())?;
        let dest = dest_hwnd(&window);

        if opts.layered {
            apply_layered(dest)?;
        } else {
            let ex = unsafe { GetWindowLongPtrW(dest, GWL_EXSTYLE) } as u32;
            println!(
                "dest {}  {}×{}  always-on-top  opaque  WS_EX_LAYERED={}",
                hwnd_hex(dest),
                window.inner_size().width,
                window.inner_size().height,
                ex & WS_EX_LAYERED.0 != 0
            );
        }

        let thumb = unsafe { DwmRegisterThumbnail(dest, src.hwnd) }.map_err(|e| {
            format!(
                "DwmRegisterThumbnail failed ({e}). If this is a code-level refusal \
                 (dest not top-level / not our process), stop — do not fake a clone. \
                 Next spike if DWM is impossible: Windows.Graphics.Capture. \
                 Do not implement capture in this crate."
            )
        })?;
        println!(
            "DwmRegisterThumbnail ok  thumb={thumb:#x}  crop={}  layered={}",
            opts.crop, opts.layered
        );

        if let Err(e) = update_thumb(thumb, dest, src.hwnd, opts.crop, !src.iconic) {
            let _ = unsafe { DwmUnregisterThumbnail(thumb) };
            return Err(format!("DwmUpdateThumbnailProperties failed ({e})"));
        }
        if src.iconic {
            paint_empty(dest, "source minimized — DWM freezes the last frame");
        }

        println!("click the mirror to raise the source. Ctrl+C / close to quit.");
        // Keep a copy for the close handler — tao owns the window.
        let src_hwnd = src.hwnd;
        let mut last_iconic = src.iconic;
        let mut last_title = src.title.clone();
        let mut dead = false;
        let crop = opts.crop;

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;
            match event {
                Event::UserEvent(()) => {
                    if dead {
                        return;
                    }
                    unsafe {
                        if !IsWindow(Some(src_hwnd)).as_bool() {
                            dead = true;
                            window.set_title(&title_for(&src, false, true));
                            let _ = DwmUnregisterThumbnail(thumb);
                            paint_empty(dest, "source closed");
                            println!("source window gone — leaving the empty dest up.");
                            return;
                        }
                    }
                    let iconic = unsafe { IsIconic(src_hwnd).as_bool() };
                    if iconic != last_iconic {
                        last_iconic = iconic;
                        window.set_title(&title_for(&src, iconic, false));
                        let _ = update_thumb(thumb, dest, src_hwnd, crop, !iconic);
                        if iconic {
                            paint_empty(dest, "source minimized — DWM freezes the last frame");
                            println!("source minimized — showing frozen/empty state.");
                        } else {
                            println!("source restored — thumbnail visible again.");
                        }
                    }
                    // Title can change as the YouTube tab switches videos.
                    if !iconic {
                        let t = utf16_text(src_hwnd, GetWindowTextW);
                        if !t.is_empty() && t != last_title {
                            last_title = t;
                            window.set_title(&format!(
                                "mirror — {}",
                                last_title.chars().take(48).collect::<String>()
                            ));
                        }
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::Resized(_),
                    ..
                } => {
                    if !dead {
                        let _ = update_thumb(thumb, dest, src_hwnd, crop, !last_iconic);
                    }
                }
                Event::WindowEvent {
                    event:
                        WindowEvent::MouseInput {
                            state: ElementState::Pressed,
                            button: MouseButton::Left,
                            ..
                        },
                    ..
                } => {
                    if !dead {
                        raise_source(src_hwnd);
                        let fg = unsafe { GetForegroundWindow() };
                        println!(
                            "click → raise source {}  foreground_now={}",
                            hwnd_hex(src_hwnd),
                            hwnd_hex(fg)
                        );
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    if let Ok(pos) = window.outer_position() {
                        save_seat(pos, window.inner_size());
                    }
                    if !dead {
                        let _ = unsafe { DwmUnregisterThumbnail(thumb) };
                    }
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_titles() {
        assert!(is_youtube_title(
            "[SONG] Burning Blue - YouTube - Google Chrome"
        ));
        assert!(is_youtube_title("YouTube Music"));
        assert!(!is_youtube_title("Roblox"));
    }

    #[test]
    fn browsers() {
        assert!(is_browser("chrome.exe", "Chrome_WidgetWin_1"));
        assert!(is_browser("msedge.exe", "Chrome_WidgetWin_1"));
        assert!(is_browser("brave.exe", "Chrome_WidgetWin_1"));
        assert!(is_browser("firefox.exe", "MozillaWindowClass"));
        assert!(!is_browser("RobloxPlayerBeta.exe", "WINDOWSCLIENT"));
    }

    #[test]
    fn cli_auto_and_flags() {
        let o = parse_opts(&[]).unwrap();
        assert_eq!(o.picker, Picker::Auto);
        assert!(!o.layered && !o.crop);

        let o = parse_opts(&["--crop".into(), "youtube".into()]).unwrap();
        assert_eq!(o.picker, Picker::Youtube);
        assert!(o.crop);

        let o = parse_opts(&["--layered".into(), "3".into()]).unwrap();
        assert_eq!(o.picker, Picker::Index(3));
        assert!(o.layered);

        let o = parse_opts(&["list".into()]).unwrap();
        assert_eq!(o.picker, Picker::List);

        let o = parse_opts(&["burning".into()]).unwrap();
        assert_eq!(o.picker, Picker::Substring("burning".into()));
    }
}
