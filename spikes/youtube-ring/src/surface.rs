//! Windows surface: frameless tao window + 12px native hit-test ring +
//! wry WebView2 inset on youtube.com + a ring-only close overlay.
//!
//! v1 (`../web-surface`) died with its parent and used stock decorations.
//! This process detaches before creating the window so a launcher/session
//! close cannot take the ring with it.

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use tao::{
    dpi::{LogicalPosition, LogicalSize, PhysicalPosition},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    platform::windows::{WindowBuilderExtWindows, WindowExtWindows},
    window::WindowBuilder,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE},
        Gdi::{
            BeginPaint, CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, EndPaint,
            GetMonitorInfoW, MonitorFromWindow, ScreenToClient, SetWindowRgn, MONITORINFO,
            MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, RGN_DIFF,
        },
    },
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetClientRect, GetCursorPos, GetWindow, GetWindowLongPtrW, GetWindowRect, KillTimer,
            SetTimer, GW_CHILD, GW_HWNDNEXT, HTCAPTION, HTCLIENT, HTTRANSPARENT,
            WINDOW_LONG_PTR_INDEX, WM_DESTROY, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_LBUTTONUP,
            WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_PAINT, WM_TIMER, WS_CAPTION,
            WS_SYSMENU, WS_THICKFRAME,
        },
    },
};
use wry::{http::Request, Rect as WebRect, WebContext, WebView, WebViewBuilder};

use crate::snap::{self, Rect as SnapRect};

const YT_URL: &str = "https://www.youtube.com/";
const OUTER_W: f64 = 664.0;
const OUTER_H: f64 = 384.0;
const VIEW_W: f64 = 640.0;
const VIEW_H: f64 = 360.0;
const RING_PX: f64 = 12.0;
const CLOSE_HIT_PX: f64 = 20.0;
const RAIL_PX: f64 = 24.0;
const RADIUS_PX: f64 = 12.0;

/// House surface (warm near-black). Window-bg only — shell + hairline paint
/// in close.html and fade via opacity. "~97%" is not restyled here.
const SURFACE: (u8, u8, u8) = (20, 18, 16);

const SUBCLASS_ID: usize = 0x5954_0001;
const CHROME_SUBCLASS_ID: usize = 0x5954_0002;
const TIMER_HOT: usize = 1;
const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
const DWMWCP_DONOTROUND: u32 = 1; // we own the 12px region
const GWL_STYLE: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX(-16);

#[derive(Clone, Debug)]
pub enum Msg {
    Close,
    Hot(bool),
    Settle,
}

struct SubState {
    proxy: EventLoopProxy<Msg>,
    probe: bool,
    last_hot: AtomicBool,
}

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let attached = args.iter().any(|a| a == "--attached");
    let surface = args.iter().any(|a| a == "--surface");
    let probe = args.iter().any(|a| a == "--probe");

    if !attached && !surface {
        match detach(&args) {
            Ok(pid) => {
                eprintln!("youtube-ring: detached pid {pid} (launcher exiting)");
                std::process::exit(0);
            }
            Err(e) => eprintln!("youtube-ring: detach failed ({e}); staying attached"),
        }
    }

    if let Err(e) = run_surface(probe) {
        eprintln!("youtube-ring: {e}");
        std::process::exit(1);
    }
}

fn detach(args: &[String]) -> std::io::Result<u32> {
    fn spawn(args: &[String], flags: u32) -> std::io::Result<u32> {
        let exe = std::env::current_exe()?;
        let mut cmd = Command::new(exe);
        cmd.arg("--surface");
        for a in args {
            if a != "--surface" && a != "--attached" {
                cmd.arg(a);
            }
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(flags);
        Ok(cmd.spawn()?.id())
    }

    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
    // | CREATE_BREAKAWAY_FROM_JOB — breakaway is the parent-job kill;
    // retry without it when the process is not in a job.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    spawn(args, base | CREATE_BREAKAWAY_FROM_JOB).or_else(|_| spawn(args, base))
}

fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("palette-youtube-ring-spike")
}

fn seat_path() -> PathBuf {
    data_dir().join("seat.txt")
}

fn probe_path() -> PathBuf {
    data_dir().join("probe.log")
}

/// Persist x,y only. Size is born 664×384 and never user-resized.
fn load_seat() -> Option<(i32, i32)> {
    let s = fs::read_to_string(seat_path()).ok()?;
    let mut it = s.split_whitespace().filter_map(|t| t.parse::<i64>().ok());
    Some((it.next()? as i32, it.next()? as i32))
}

fn save_seat(pos: PhysicalPosition<i32>) {
    let _ = fs::create_dir_all(data_dir());
    let _ = fs::write(seat_path(), format!("{} {}", pos.x, pos.y));
}

fn probe_log(line: &str) {
    let _ = fs::create_dir_all(data_dir());
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(probe_path())
    {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "{ts} {line}");
    }
    eprintln!("probe: {line}");
}

fn run_surface(probe: bool) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let mut builder = WindowBuilder::new()
        .with_title("youtube-ring")
        .with_always_on_top(true)
        .with_decorations(false)
        .with_resizable(false)
        .with_maximizable(false)
        .with_transparent(false)
        .with_background_color((SURFACE.0, SURFACE.1, SURFACE.2, 255))
        .with_inner_size(LogicalSize::new(OUTER_W, OUTER_H))
        .with_min_inner_size(LogicalSize::new(OUTER_W, OUTER_H))
        .with_max_inner_size(LogicalSize::new(OUTER_W, OUTER_H))
        .with_visible(false)
        .with_undecorated_shadow(false);
    if let Some((x, y)) = load_seat() {
        builder = builder.with_position(PhysicalPosition::new(x, y));
    }

    let window = builder.build(&event_loop).map_err(|e| e.to_string())?;
    let hwnd = HWND(window.hwnd() as *mut _);

    kill_dwm_accent(hwnd);
    probe_styles(hwnd, probe, "post-create");

    let scale = window.scale_factor();
    apply_round_region(hwnd, scale);

    let yt_bounds = WebRect {
        position: LogicalPosition::new(RING_PX, RING_PX).into(),
        size: LogicalSize::new(VIEW_W, VIEW_H).into(),
    };
    let close_bounds = WebRect {
        position: LogicalPosition::new(0.0, 0.0).into(),
        size: LogicalSize::new(OUTER_W, OUTER_H).into(),
    };

    let mut web_context = WebContext::new(Some(data_dir().join("profile")));
    let youtube = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_url(YT_URL)
        .with_bounds(yt_bounds)
        .with_background_color((0, 0, 0, 255))
        .with_focused(true)
        .build_as_child(&window)?;

    let before_close = child_hwnds(hwnd);
    let close = WebViewBuilder::new()
        .with_html(include_str!("close.html"))
        .with_bounds(close_bounds)
        .with_transparent(true)
        .with_background_color((0, 0, 0, 0))
        .with_devtools(false)
        .with_focused(false)
        .with_ipc_handler({
            let proxy = proxy.clone();
            move |req: Request<String>| {
                if req.body() == "close" {
                    let _ = proxy.send_event(Msg::Close);
                }
            }
        })
        .build_as_child(&window)?;

    let close_hwnd = newest_child(hwnd, &before_close).unwrap_or_default();
    apply_ring_frame_region(close_hwnd, scale);
    install_chrome_ht(close_hwnd);
    let _ = youtube.set_bounds(yt_bounds);
    let _ = close.set_bounds(close_bounds);

    install_subclass(hwnd, proxy.clone(), probe);
    if probe {
        dump_bounds_probe(&window, &youtube, &close);
    }

    settle_window(&window);
    let _ = window.set_visible(true);

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _keep_yt = &youtube;
        let _keep_close = &close;
        match event {
            Event::UserEvent(Msg::Close) => {
                if let Ok(pos) = window.outer_position() {
                    save_seat(pos);
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(Msg::Hot(hot)) => {
                let js = if hot {
                    "document.documentElement.setAttribute('data-hot','')"
                } else {
                    "document.documentElement.removeAttribute('data-hot')"
                };
                let _ = close.evaluate_script(js);
                // WebView2 may spawn inner HWNDs after first navigate.
                install_chrome_ht(close_hwnd);
                if probe {
                    probe_log(&format!(
                        "paint/hot chrome_visible={} (focus-visible is CSS)",
                        snap::chrome_visible(hot, false)
                    ));
                }
            }
            Event::UserEvent(Msg::Settle) => settle_window(&window),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if let Ok(pos) = window.outer_position() {
                    save_seat(pos);
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                let scale = window.scale_factor();
                apply_round_region(hwnd, scale);
                apply_ring_frame_region(close_hwnd, scale);
                let _ = youtube.set_bounds(yt_bounds);
                let _ = close.set_bounds(close_bounds);
                if probe {
                    dump_bounds_probe(&window, &youtube, &close);
                }
            }
            _ => {}
        }
    });
}

fn kill_dwm_accent(hwnd: HWND) {
    let color = DWMWA_COLOR_NONE;
    let corner = DWMWCP_DONOTROUND;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            std::ptr::from_ref(&color).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corner).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
}

fn probe_styles(hwnd: HWND, probe: bool, tag: &str) {
    if !probe {
        return;
    }
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let mut wr = RECT::default();
    let mut cr = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut wr) };
    let _ = unsafe { GetClientRect(hwnd, &mut cr) };
    let frame_h = (wr.bottom - wr.top) - (cr.bottom - cr.top);
    let frame_w = (wr.right - wr.left) - (cr.right - cr.left);
    probe_log(&format!(
        "{tag} style=0x{style:08x} caption={} thickframe={} sysmenu={} frame_delta={}x{}",
        style & WS_CAPTION.0 != 0,
        style & WS_THICKFRAME.0 != 0,
        style & WS_SYSMENU.0 != 0,
        frame_w,
        frame_h
    ));
}

fn dump_bounds_probe(window: &tao::window::Window, youtube: &WebView, close: &WebView) {
    let inner = window.inner_size();
    let yt = youtube.bounds().ok();
    let cl = close.bounds().ok();
    probe_log(&format!(
        "inner={}x{} yt={yt:?} chrome={cl:?} expect_inset=12,12,640,360 chrome=664x384 ring-region, close_hit=20x20@top-right L",
        inner.width, inner.height
    ));
}

fn child_hwnds(parent: HWND) -> Vec<HWND> {
    let mut out = Vec::new();
    let mut h = unsafe { GetWindow(parent, GW_CHILD) }.unwrap_or_default();
    while !h.is_invalid() {
        out.push(h);
        h = unsafe { GetWindow(h, GW_HWNDNEXT) }.unwrap_or_default();
    }
    out
}

fn newest_child(parent: HWND, before: &[HWND]) -> Option<HWND> {
    let before_ids: Vec<isize> = before.iter().map(|h| h.0 as isize).collect();
    child_hwnds(parent)
        .into_iter()
        .find(|h| !before_ids.contains(&(h.0 as isize)))
}

fn apply_round_region(hwnd: HWND, scale: f64) {
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let dia = ((RADIUS_PX * scale).round() as i32 * 2).max(2);
    let rgn = unsafe { CreateRoundRectRgn(0, 0, rc.right + 1, rc.bottom + 1, dia, dia) };
    let _ = unsafe { SetWindowRgn(hwnd, Some(rgn), true) };
}

/// Chrome HWND is the 12px frame only — never the video inset.
fn apply_ring_frame_region(hwnd: HWND, scale: f64) {
    if hwnd.is_invalid() {
        return;
    }
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let ring = (RING_PX * scale).round() as i32;
    unsafe {
        let outer = CreateRectRgn(0, 0, rc.right, rc.bottom);
        let inner = CreateRectRgn(ring, ring, rc.right - ring, rc.bottom - ring);
        let dest = CreateRectRgn(0, 0, 0, 0);
        let _ = CombineRgn(Some(dest), Some(outer), Some(inner), RGN_DIFF);
        let _ = DeleteObject(outer.into());
        let _ = DeleteObject(inner.into());
        let _ = SetWindowRgn(hwnd, Some(dest), true);
    }
}

/// Chrome paints the ring; hits on the frame fall through to the parent
/// HTCAPTION except the close L (HTCLIENT). HTCAPTION stays always-on.
fn install_chrome_ht(root: HWND) {
    if root.is_invalid() {
        return;
    }
    let mut stack = vec![root];
    while let Some(h) = stack.pop() {
        let _ = unsafe { SetWindowSubclass(h, Some(chrome_ht_proc), CHROME_SUBCLASS_ID, 0) };
        let mut c = unsafe { GetWindow(h, GW_CHILD) }.unwrap_or_default();
        while !c.is_invalid() {
            stack.push(c);
            c = unsafe { GetWindow(c, GW_HWNDNEXT) }.unwrap_or_default();
        }
    }
}

unsafe extern "system" fn chrome_ht_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_NCHITTEST {
        let pt = lparam_client(hwnd, lparam);
        let (w, h) = client_size(hwnd);
        let scale = scale_of(hwnd);
        let ring = (RING_PX * scale).round() as i32;
        let hit = (CLOSE_HIT_PX * scale).round() as i32;
        if snap::in_close_l(pt.x, pt.y, w, h, ring, hit) {
            return LRESULT(HTCLIENT as isize);
        }
        return LRESULT(HTTRANSPARENT as isize);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

fn monitor_rects(hwnd: HWND) -> Option<(SnapRect, SnapRect)> {
    let mon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(mon, &mut info) }.as_bool() {
        return None;
    }
    let to = |r: RECT| SnapRect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    };
    Some((to(info.rcMonitor), to(info.rcWork)))
}

fn settle_window(window: &tao::window::Window) {
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let size = window.outer_size();
    let hwnd = HWND(window.hwnd() as *mut _);
    let Some((mon, work)) = monitor_rects(hwnd) else {
        save_seat(pos);
        return;
    };
    let rail = (RAIL_PX * window.scale_factor()).round() as i32;
    let (x, y) = snap::settle(
        pos.x,
        pos.y,
        size.width as i32,
        size.height as i32,
        mon,
        work,
        rail,
    );
    let _ = window.set_outer_position(PhysicalPosition::new(x, y));
    save_seat(PhysicalPosition::new(x, y));
}

fn install_subclass(hwnd: HWND, proxy: EventLoopProxy<Msg>, probe: bool) {
    let state = Box::new(SubState {
        proxy,
        probe,
        last_hot: AtomicBool::new(false),
    });
    let raw = Box::into_raw(state);
    let ok = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, raw as usize) };
    if !ok.as_bool() {
        unsafe {
            drop(Box::from_raw(raw));
        }
        return;
    }
    let _ = unsafe { SetTimer(Some(hwnd), TIMER_HOT, 50, None) };
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let state = unsafe { &*(data as *const SubState) };
    match msg {
        WM_NCHITTEST => {
            let ht = hit_test(hwnd, lparam);
            if state.probe {
                static LAST: AtomicI32 = AtomicI32::new(i32::MIN);
                if LAST.swap(ht, Ordering::Relaxed) != ht {
                    probe_log(&format!(
                        "nchittest {ht} (caption={HTCAPTION} client={HTCLIENT})"
                    ));
                }
            }
            return LRESULT(ht as isize);
        }
        WM_NCLBUTTONDBLCLK => {
            // HTCAPTION dblclick would maximize. Size is born; eat it.
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            if cursor_in_close_l(hwnd) {
                let _ = state.proxy.send_event(Msg::Close);
                return LRESULT(0);
            }
        }
        WM_EXITSIZEMOVE => {
            let _ = state.proxy.send_event(Msg::Settle);
        }
        WM_TIMER if wparam.0 == TIMER_HOT => {
            let hot = cursor_in_ring(hwnd);
            if state.last_hot.swap(hot, Ordering::Relaxed) != hot {
                let _ = state.proxy.send_event(Msg::Hot(hot));
            }
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            // Chrome (shell + hairline + ×) is CSS opacity on the overlay.
            // Complete the paint cycle; do not FillRect SURFACE — that was
            // the always-on brown ring leftover. Not gated on hot.
            paint_ring(hwnd);
            return LRESULT(0);
        }
        WM_NCCALCSIZE => {
            // Frameless: no non-client chrome to compute. Fall through.
        }
        WM_DESTROY => {
            let _ = KillTimer(Some(hwnd), TIMER_HOT);
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
            drop(Box::from_raw(data as *mut SubState));
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rc = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rc) };
    (rc.right, rc.bottom)
}

fn scale_of(hwnd: HWND) -> f64 {
    // Prefer the window's own DPI so the 12/20/24 stamp numbers stay logical.
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        1.0
    } else {
        dpi as f64 / 96.0
    }
}

fn cursor_client(hwnd: HWND) -> Option<(i32, i32)> {
    let mut pt = POINT::default();
    unsafe { GetCursorPos(&mut pt) }.ok()?;
    if !unsafe { ScreenToClient(hwnd, &mut pt) }.as_bool() {
        return None;
    }
    Some((pt.x, pt.y))
}

fn cursor_in_ring(hwnd: HWND) -> bool {
    let Some((x, y)) = cursor_client(hwnd) else {
        return false;
    };
    let (w, h) = client_size(hwnd);
    let ring = (RING_PX * scale_of(hwnd)).round() as i32;
    snap::in_ring(x, y, w, h, ring)
}

fn cursor_in_close_l(hwnd: HWND) -> bool {
    let Some((x, y)) = cursor_client(hwnd) else {
        return false;
    };
    let (w, h) = client_size(hwnd);
    let scale = scale_of(hwnd);
    snap::in_close_l(
        x,
        y,
        w,
        h,
        (RING_PX * scale).round() as i32,
        (CLOSE_HIT_PX * scale).round() as i32,
    )
}

fn lparam_client(hwnd: HWND, lparam: LPARAM) -> POINT {
    let x = (lparam.0 as i32) as i16 as i32;
    let y = ((lparam.0 as i32) >> 16) as i16 as i32;
    let mut pt = POINT { x, y };
    let _ = unsafe { ScreenToClient(hwnd, &mut pt) };
    pt
}

fn hit_test(hwnd: HWND, lparam: LPARAM) -> i32 {
    let pt = lparam_client(hwnd, lparam);
    let (w, h) = client_size(hwnd);
    let scale = scale_of(hwnd);
    let ring = (RING_PX * scale).round() as i32;
    let hit = (CLOSE_HIT_PX * scale).round() as i32;
    if snap::in_close_l(pt.x, pt.y, w, h, ring, hit) {
        return HTCLIENT as i32;
    }
    if snap::in_ring(pt.x, pt.y, w, h, ring) {
        return HTCAPTION as i32;
    }
    HTCLIENT as i32
}

fn paint_ring(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    if hdc.is_invalid() {
        return;
    }
    let _ = unsafe { EndPaint(hwnd, &ps) };
}
