// MouseSpot - mouse/key highlight overlay for presentations & screencasts
// Phase 0/1 prototype: transparent click-through overlay, cursor highlight, click ripples.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::time::Instant;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Controls::{InitCommonControlsEx, ICC_BAR_CLASSES, ICC_TAB_CLASSES, INITCOMMONCONTROLSEX};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const TIMER_ID: usize = 1;
const WM_TRAY: u32 = WM_APP + 1;
const HOTKEY_TOGGLE: i32 = 1;
const HOTKEY_SPOT: i32 = 2;
const CMD_TOGGLE: usize = 100;
const CMD_EXIT: usize = 101;
const CMD_SETTINGS: usize = 102;
const CMD_SPOT: usize = 103;

// trackbar / button messages (avoid relying on crate constants)
const WM_USER_: u32 = 0x0400;
const TBM_GETPOS: u32 = WM_USER_;
const TBM_SETPOS: u32 = WM_USER_ + 5;
const TBM_SETRANGE: u32 = WM_USER_ + 6;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const TB_THUMBTRACK: u32 = 5; // continuous drag notification (don't persist mid-drag)
const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETCURSEL: u32 = 0x0147;
const CBN_SELCHANGE: u32 = 1;
// tab control
const ID_TAB: i32 = 2500;
const TCM_FIRST: u32 = 0x1300;
const TCM_INSERTITEMW: u32 = TCM_FIRST + 62;
const TCM_GETCURSEL: u32 = TCM_FIRST + 11;
const TCIF_TEXT: u32 = 0x0001;
const TCN_SELCHANGE: u32 = (-551i32) as u32;

#[repr(C)]
struct TcItem {
    mask: u32,
    dw_state: u32,
    dw_state_mask: u32,
    psz_text: *mut u16,
    cch_text_max: i32,
    i_image: i32,
    l_param: isize,
}

#[repr(C)]
struct Nmhdr {
    hwnd_from: HWND,
    id_from: usize,
    code: u32,
}

// settings control IDs
const ID_HL_RADIUS: i32 = 2001;
const ID_HL_FILL_A: i32 = 2002;
const ID_RING_A: i32 = 2003;
const ID_RIPPLE_R: i32 = 2004;
const ID_RIPPLE_MS: i32 = 2005;
const ID_KEY_FONT: i32 = 2006;
const ID_KEY_MS: i32 = 2007;
const ID_KEY_GAP: i32 = 2008;
const ID_DRAG_R: i32 = 2009;
const ID_DRAG_MS: i32 = 2010;
const ID_SCROLL_MS: i32 = 2011;
const ID_KEY_X: i32 = 2012;
const ID_KEY_Y: i32 = 2013;
const ID_SPOT_RADIUS: i32 = 2014;
const ID_SPOT_DIM: i32 = 2015;
const ID_SPOT_FEATHER: i32 = 2016;
const ID_KEY_POS: i32 = 2401;
const ID_KEY_STACK: i32 = 2101;
const ID_SHOW_ALL: i32 = 2102;
const ID_SHOW_DRAG: i32 = 2103;
const ID_SHOW_SCROLL: i32 = 2104;
const ID_SPOT_ON: i32 = 2105;
const ID_COL_FILL: i32 = 2201;
const ID_COL_RING: i32 = 2202;
const ID_COL_LEFT: i32 = 2203;
const ID_COL_RIGHT: i32 = 2204;
const ID_COL_MIDDLE: i32 = 2207;
const ID_COL_DOUBLE: i32 = 2208;
const ID_COL_SCROLL: i32 = 2209;
const ID_COL_DRAG: i32 = 2210;
const ID_COL_KEYBG: i32 = 2205;
const ID_COL_KEYTX: i32 = 2206;
const ID_RESET: i32 = 2302;
const ID_CLOSE: i32 = 2303;

// (id, label, min, max, page)  page 0 = マウス操作, 1 = キー操作
const SLIDERS: &[(i32, &str, i32, i32, i32)] = &[
    (ID_HL_RADIUS, "ハイライト直径", 10, 120, 0),
    (ID_HL_FILL_A, "ハイライト塗り 不透明度", 0, 255, 0),
    (ID_RING_A, "リング 不透明度", 0, 255, 0),
    (ID_RIPPLE_R, "波紋 最大半径", 20, 160, 0),
    (ID_RIPPLE_MS, "波紋 表示時間(ms)", 150, 1500, 0),
    (ID_DRAG_R, "ドラッグ 点サイズ", 2, 16, 0),
    (ID_DRAG_MS, "ドラッグ 残り時間(ms)", 200, 2000, 0),
    (ID_SCROLL_MS, "スクロール 表示時間(ms)", 200, 1500, 0),
    (ID_SPOT_RADIUS, "スポット 半径", 40, 400, 0),
    (ID_SPOT_DIM, "スポット 暗さ", 0, 240, 0),
    (ID_SPOT_FEATHER, "スポット 縁ぼかし", 0, 120, 0),
    (ID_KEY_FONT, "キー 文字サイズ", 16, 60, 1),
    (ID_KEY_MS, "キー 表示時間(ms)", 500, 4000, 1),
    (ID_KEY_GAP, "キー 間隔", 0, 40, 1),
    (ID_KEY_X, "キー位置 X (座標指定時)", 0, 3840, 1),
    (ID_KEY_Y, "キー位置 Y (座標指定時)", 0, 2160, 1),
];

// (id, label, page) for color buttons
const COLORS: &[(i32, &str, i32)] = &[
    (ID_COL_FILL, "ハイライト色", 0),
    (ID_COL_RING, "リング色", 0),
    (ID_COL_LEFT, "左クリック色", 0),
    (ID_COL_RIGHT, "右クリック色", 0),
    (ID_COL_MIDDLE, "中クリック色", 0),
    (ID_COL_DOUBLE, "ダブルクリック色", 0),
    (ID_COL_SCROLL, "スクロール色", 0),
    (ID_COL_DRAG, "ドラッグ軌跡色", 0),
    (ID_COL_KEYBG, "キー背景色", 1),
    (ID_COL_KEYTX, "キー文字色", 1),
];

type Rgba = (u8, u8, u8, u8);

#[derive(Clone)]
struct Config {
    highlight_radius: f32,
    highlight_fill: Rgba,
    highlight_ring: Rgba,
    ring_thickness: f32,
    ripple_lifetime_ms: f32,
    ripple_start_r: f32,
    ripple_end_r: f32,
    ripple_thickness: f32,
    left_color: Rgba,
    right_color: Rgba,
    middle_color: Rgba,
    double_color: Rgba,
    scroll_color: Rgba,
    drag_color: Rgba,
    show_scroll: bool,
    show_drag: bool,
    drag_dot_radius: f32,
    drag_lifetime_ms: f32,
    scroll_lifetime_ms: f32,
    // spotlight (dim) mode
    spotlight_enabled: bool,
    spotlight_radius: f32,
    spotlight_dim: u8,
    spotlight_feather: f32,
    // key-stroke toast
    key_font_px: i32,
    key_bg: Rgba,
    key_text_color: Rgba,
    key_lifetime_ms: f32,
    key_fade_ms: f32,
    key_margin_bottom: i32,
    key_padding: (i32, i32),
    key_radius: f32,
    key_pos: i32, // 0:下中央 1:下左 2:下右 3:上中央 4:上左 5:上右 6:中央 7:座標指定
    key_x: i32,   // key_pos=7 のときの左上X
    key_y: i32,   // key_pos=7 のときの左上Y
    show_all_keys: bool,
    key_stack: bool, // true: 追加型（右へ並べる）, false: 上書き型
    key_gap: i32,    // スタック時のキー間隔(px)
    key_max: usize,  // 同時表示する最大キー数
}

impl Default for Config {
    fn default() -> Self {
        Config {
            highlight_radius: 34.0,
            highlight_fill: (255, 220, 40, 55),
            highlight_ring: (255, 210, 0, 150),
            ring_thickness: 2.5,
            ripple_lifetime_ms: 480.0,
            ripple_start_r: 6.0,
            ripple_end_r: 64.0,
            ripple_thickness: 4.0,
            left_color: (255, 235, 80, 210),
            right_color: (90, 200, 255, 210),
            middle_color: (180, 120, 255, 210),
            double_color: (120, 255, 140, 220),
            scroll_color: (255, 255, 255, 220),
            drag_color: (255, 200, 60, 160),
            show_scroll: true,
            show_drag: true,
            drag_dot_radius: 5.0,
            drag_lifetime_ms: 600.0,
            scroll_lifetime_ms: 500.0,
            spotlight_enabled: false,
            spotlight_radius: 120.0,
            spotlight_dim: 160,
            spotlight_feather: 40.0,
            key_font_px: 30,
            key_bg: (20, 20, 24, 225),
            key_text_color: (255, 255, 255, 255),
            key_lifetime_ms: 1500.0,
            key_fade_ms: 300.0,
            key_margin_bottom: 90,
            key_padding: (22, 12),
            key_radius: 12.0,
            key_pos: 0,
            key_x: 200,
            key_y: 200,
            show_all_keys: true,
            key_stack: true,
            key_gap: 8,
            key_max: 24,
        }
    }
}

fn parse_rgba(v: &str) -> Option<Rgba> {
    let n: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if n.len() == 4 {
        Some((n[0], n[1], n[2], n[3]))
    } else {
        None
    }
}

fn parse_2i(v: &str) -> Option<(i32, i32)> {
    let n: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if n.len() == 2 {
        Some((n[0], n[1]))
    } else {
        None
    }
}

// remembered settings-window position (kept separate from config.toml)
fn win_pos_path() -> Option<std::path::PathBuf> {
    let mut p = Config::config_path()?;
    p.set_file_name("window.txt");
    Some(p)
}

fn load_win_pos() -> Option<(i32, i32)> {
    let s = std::fs::read_to_string(win_pos_path()?).ok()?;
    let (a, b) = s.trim().split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn save_win_pos(x: i32, y: i32) {
    if let Some(p) = win_pos_path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, format!("{},{}", x, y));
    }
}

fn clamp_pos(x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    unsafe {
        let vl = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vt = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vr = vl + GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vb = vt + GetSystemMetrics(SM_CYVIRTUALSCREEN);
        (x.max(vl).min((vr - w).max(vl)), y.max(vt).min((vb - h).max(vt)))
    }
}

impl Config {
    fn config_path() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("APPDATA")?;
        let mut p = std::path::PathBuf::from(base);
        p.push("MouseSpot");
        p.push("config.toml");
        Some(p)
    }

    fn load() -> Config {
        let mut c = Config::default();
        if let Some(p) = Self::config_path() {
            if let Ok(s) = std::fs::read_to_string(&p) {
                c.apply_toml(&s);
            }
        }
        c
    }

    fn save(&self) {
        if let Some(p) = Self::config_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&p, self.to_toml());
        }
    }

    fn to_toml(&self) -> String {
        fn c(x: Rgba) -> String {
            format!("{},{},{},{}", x.0, x.1, x.2, x.3)
        }
        format!(
            "# MouseSpot config\n\
             highlight_radius = {}\n\
             highlight_fill = {}\n\
             highlight_ring = {}\n\
             ring_thickness = {}\n\
             ripple_lifetime_ms = {}\n\
             ripple_start_r = {}\n\
             ripple_end_r = {}\n\
             ripple_thickness = {}\n\
             left_color = {}\n\
             right_color = {}\n\
             middle_color = {}\n\
             double_color = {}\n\
             scroll_color = {}\n\
             drag_color = {}\n\
             show_scroll = {}\n\
             show_drag = {}\n\
             drag_dot_radius = {}\n\
             drag_lifetime_ms = {}\n\
             scroll_lifetime_ms = {}\n\
             spotlight_enabled = {}\n\
             spotlight_radius = {}\n\
             spotlight_dim = {}\n\
             spotlight_feather = {}\n\
             key_font_px = {}\n\
             key_bg = {}\n\
             key_text_color = {}\n\
             key_lifetime_ms = {}\n\
             key_fade_ms = {}\n\
             key_margin_bottom = {}\n\
             key_padding = {},{}\n\
             key_radius = {}\n\
             key_pos = {}\n\
             key_x = {}\n\
             key_y = {}\n\
             show_all_keys = {}\n\
             key_stack = {}\n\
             key_gap = {}\n\
             key_max = {}\n",
            self.highlight_radius,
            c(self.highlight_fill),
            c(self.highlight_ring),
            self.ring_thickness,
            self.ripple_lifetime_ms,
            self.ripple_start_r,
            self.ripple_end_r,
            self.ripple_thickness,
            c(self.left_color),
            c(self.right_color),
            c(self.middle_color),
            c(self.double_color),
            c(self.scroll_color),
            c(self.drag_color),
            self.show_scroll,
            self.show_drag,
            self.drag_dot_radius,
            self.drag_lifetime_ms,
            self.scroll_lifetime_ms,
            self.spotlight_enabled,
            self.spotlight_radius,
            self.spotlight_dim,
            self.spotlight_feather,
            self.key_font_px,
            c(self.key_bg),
            c(self.key_text_color),
            self.key_lifetime_ms,
            self.key_fade_ms,
            self.key_margin_bottom,
            self.key_padding.0,
            self.key_padding.1,
            self.key_radius,
            self.key_pos,
            self.key_x,
            self.key_y,
            self.show_all_keys,
            self.key_stack,
            self.key_gap,
            self.key_max,
        )
    }

    fn apply_toml(&mut self, s: &str) {
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "highlight_radius" => {
                    if let Ok(x) = v.parse() {
                        self.highlight_radius = x;
                    }
                }
                "highlight_fill" => {
                    if let Some(c) = parse_rgba(v) {
                        self.highlight_fill = c;
                    }
                }
                "highlight_ring" => {
                    if let Some(c) = parse_rgba(v) {
                        self.highlight_ring = c;
                    }
                }
                "ring_thickness" => {
                    if let Ok(x) = v.parse() {
                        self.ring_thickness = x;
                    }
                }
                "ripple_lifetime_ms" => {
                    if let Ok(x) = v.parse() {
                        self.ripple_lifetime_ms = x;
                    }
                }
                "ripple_start_r" => {
                    if let Ok(x) = v.parse() {
                        self.ripple_start_r = x;
                    }
                }
                "ripple_end_r" => {
                    if let Ok(x) = v.parse() {
                        self.ripple_end_r = x;
                    }
                }
                "ripple_thickness" => {
                    if let Ok(x) = v.parse() {
                        self.ripple_thickness = x;
                    }
                }
                "left_color" => {
                    if let Some(c) = parse_rgba(v) {
                        self.left_color = c;
                    }
                }
                "right_color" => {
                    if let Some(c) = parse_rgba(v) {
                        self.right_color = c;
                    }
                }
                "middle_color" => {
                    if let Some(c) = parse_rgba(v) {
                        self.middle_color = c;
                    }
                }
                "double_color" => {
                    if let Some(c) = parse_rgba(v) {
                        self.double_color = c;
                    }
                }
                "scroll_color" => {
                    if let Some(c) = parse_rgba(v) {
                        self.scroll_color = c;
                    }
                }
                "drag_color" => {
                    if let Some(c) = parse_rgba(v) {
                        self.drag_color = c;
                    }
                }
                "show_scroll" => self.show_scroll = v == "true",
                "show_drag" => self.show_drag = v == "true",
                "drag_dot_radius" => {
                    if let Ok(x) = v.parse() {
                        self.drag_dot_radius = x;
                    }
                }
                "drag_lifetime_ms" => {
                    if let Ok(x) = v.parse() {
                        self.drag_lifetime_ms = x;
                    }
                }
                "scroll_lifetime_ms" => {
                    if let Ok(x) = v.parse() {
                        self.scroll_lifetime_ms = x;
                    }
                }
                "spotlight_enabled" => self.spotlight_enabled = v == "true",
                "spotlight_radius" => {
                    if let Ok(x) = v.parse() {
                        self.spotlight_radius = x;
                    }
                }
                "spotlight_dim" => {
                    if let Ok(x) = v.parse() {
                        self.spotlight_dim = x;
                    }
                }
                "spotlight_feather" => {
                    if let Ok(x) = v.parse() {
                        self.spotlight_feather = x;
                    }
                }
                "key_font_px" => {
                    if let Ok(x) = v.parse() {
                        self.key_font_px = x;
                    }
                }
                "key_bg" => {
                    if let Some(c) = parse_rgba(v) {
                        self.key_bg = c;
                    }
                }
                "key_text_color" => {
                    if let Some(c) = parse_rgba(v) {
                        self.key_text_color = c;
                    }
                }
                "key_lifetime_ms" => {
                    if let Ok(x) = v.parse() {
                        self.key_lifetime_ms = x;
                    }
                }
                "key_fade_ms" => {
                    if let Ok(x) = v.parse() {
                        self.key_fade_ms = x;
                    }
                }
                "key_margin_bottom" => {
                    if let Ok(x) = v.parse() {
                        self.key_margin_bottom = x;
                    }
                }
                "key_padding" => {
                    if let Some(p) = parse_2i(v) {
                        self.key_padding = p;
                    }
                }
                "key_radius" => {
                    if let Ok(x) = v.parse() {
                        self.key_radius = x;
                    }
                }
                "key_pos" => {
                    if let Ok(x) = v.parse() {
                        self.key_pos = x;
                    }
                }
                "key_x" => {
                    if let Ok(x) = v.parse() {
                        self.key_x = x;
                    }
                }
                "key_y" => {
                    if let Ok(x) = v.parse() {
                        self.key_y = x;
                    }
                }
                "show_all_keys" => self.show_all_keys = v == "true",
                "key_stack" => self.key_stack = v == "true",
                "key_gap" => {
                    if let Ok(x) = v.parse() {
                        self.key_gap = x;
                    }
                }
                "key_max" => {
                    if let Ok(x) = v.parse() {
                        self.key_max = x;
                    }
                }
                _ => {}
            }
        }
    }
}

struct Ripple {
    x: i32,
    y: i32,
    start: Instant,
    color: Rgba,
    double: bool,
}

struct ScrollHint {
    x: i32,
    y: i32,
    up: bool,
    start: Instant,
}

struct TrailDot {
    x: i32,
    y: i32,
    start: Instant,
}

struct KeyEntry {
    text: String,
    shown_at: Instant,
    mask: Option<(i32, i32, Vec<u8>)>, // cached (tw, th, coverage)
}

struct App {
    hwnd: HWND,
    keys_hwnd: HWND,
    spot_hwnd: HWND,
    spot_buf: Vec<u32>,
    spot_visible: bool,
    spot_last: (i32, i32),
    settings_hwnd: HWND,
    settings_pages: [Vec<HWND>; 2],
    hinstance: HINSTANCE,
    cfg: Config,
    enabled: bool,
    cursor: (i32, i32),
    ripples: Vec<Ripple>,
    scrolls: Vec<ScrollHint>,
    trail: Vec<TrailDot>,
    buttons_down: u8,
    last_left: Option<(Instant, (i32, i32))>,
    vs: (i32, i32, i32, i32), // left, top, right, bottom
    last_render: Instant,
    keys: Vec<KeyEntry>,
    keys_visible: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = RefCell::new(None);
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinst = GetModuleHandleW(None)?;
        let hinstance = HINSTANCE(hinst.0);
        let class_name = w!("MouseSpotOverlay");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);

        // common controls (trackbars) + settings window class
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES | ICC_TAB_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);

        let settings_class = w!("MouseSpotSettings");
        let dlg_brush = HBRUSH((COLOR_BTNFACE.0 + 1) as usize as *mut core::ffi::c_void);
        let wc2 = WNDCLASSW {
            lpfnWndProc: Some(settings_wndproc),
            hInstance: hinstance,
            lpszClassName: settings_class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: dlg_brush,
            ..Default::default()
        };
        RegisterClassW(&wc2);

        // Full-screen spotlight (dim) overlay — created first so it sits BELOW the
        // highlight/key windows in the topmost z-band.
        let spot_hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("MouseSpotSpot"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            hinstance,
            None,
        )?;

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("MouseSpot"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            hinstance,
            None,
        )?;

        // Separate overlay window for the key-stroke toast (fixed corner).
        let keys_hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("MouseSpotKeys"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            hinstance,
            None,
        )?;

        let vs = virtual_screen();
        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                hwnd,
                keys_hwnd,
                spot_hwnd,
                spot_buf: Vec::new(),
                spot_visible: false,
                spot_last: (i32::MIN, i32::MIN),
                settings_hwnd: HWND(std::ptr::null_mut()),
                settings_pages: [Vec::new(), Vec::new()],
                hinstance,
                cfg: Config::load(),
                enabled: true,
                cursor: cursor_pos(),
                ripples: Vec::new(),
                scrolls: Vec::new(),
                trail: Vec::new(),
                buttons_down: 0,
                last_left: None,
                vs,
                last_render: Instant::now(),
                keys: Vec::new(),
                keys_visible: false,
            });
        });

        add_tray(hwnd);
        let _ = RegisterHotKey(hwnd, HOTKEY_TOGGLE, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'H' as u32);
        let _ = RegisterHotKey(hwnd, HOTKEY_SPOT, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'S' as u32);

        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), hinstance, 0)?;
        let kbd_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), hinstance, 0)?;

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, TIMER_ID, 16, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnhookWindowsHookEx(hook);
        let _ = UnhookWindowsHookEx(kbd_hook);
        Ok(())
    }
}

fn virtual_screen() -> (i32, i32, i32, i32) {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        (x, y, x + w, y + h)
    }
}

fn cursor_pos() -> (i32, i32) {
    unsafe {
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        (p.x, p.y)
    }
}

fn push_ripple(app: &mut App, pt: POINT, color: Rgba, double: bool) {
    app.ripples.push(Ripple {
        x: pt.x,
        y: pt.y,
        start: Instant::now(),
        color,
        double,
    });
}

extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let ms = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let pt = ms.pt;
        let mouse_data = ms.mouseData;
        APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                if app.enabled {
                    match wparam.0 as u32 {
                        WM_MOUSEMOVE => {
                            app.cursor = (pt.x, pt.y);
                            // drag trail while a button is held
                            if app.cfg.show_drag && app.buttons_down > 0 {
                                app.trail.push(TrailDot { x: pt.x, y: pt.y, start: Instant::now() });
                            }
                            // Render straight off the move event (throttled) to minimise
                            // the perceived lag behind the hardware cursor.
                            if Instant::now().duration_since(app.last_render)
                                >= std::time::Duration::from_millis(7)
                            {
                                render(app);
                                render_spot(app);
                            }
                        }
                        WM_LBUTTONDOWN => {
                            app.cursor = (pt.x, pt.y);
                            app.buttons_down += 1;
                            // double-click detection
                            let now = Instant::now();
                            let dt = unsafe { GetDoubleClickTime() };
                            let is_double = match app.last_left {
                                Some((t, (lx, ly))) => {
                                    now.duration_since(t).as_millis() <= dt as u128
                                        && (pt.x - lx).abs() <= 6
                                        && (pt.y - ly).abs() <= 6
                                }
                                None => false,
                            };
                            if is_double {
                                let c = app.cfg.double_color;
                                push_ripple(app, pt, c, true);
                                app.last_left = None;
                            } else {
                                let c = app.cfg.left_color;
                                push_ripple(app, pt, c, false);
                                app.last_left = Some((now, (pt.x, pt.y)));
                            }
                            render(app);
                        }
                        WM_RBUTTONDOWN => {
                            app.cursor = (pt.x, pt.y);
                            app.buttons_down += 1;
                            let c = app.cfg.right_color;
                            push_ripple(app, pt, c, false);
                            render(app);
                        }
                        WM_MBUTTONDOWN => {
                            app.cursor = (pt.x, pt.y);
                            app.buttons_down += 1;
                            let c = app.cfg.middle_color;
                            push_ripple(app, pt, c, false);
                            render(app);
                        }
                        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
                            app.buttons_down = app.buttons_down.saturating_sub(1);
                        }
                        WM_MOUSEWHEEL => {
                            if app.cfg.show_scroll {
                                let delta = ((mouse_data >> 16) & 0xffff) as i16;
                                app.scrolls.push(ScrollHint {
                                    x: pt.x,
                                    y: pt.y,
                                    up: delta > 0,
                                    start: Instant::now(),
                                });
                                render(app);
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER => {
                APP.with(|a| {
                    if let Some(app) = a.borrow_mut().as_mut() {
                        render(app);
                        render_keys(app);
                        render_spot(app);
                    }
                });
                LRESULT(0)
            }
            WM_HOTKEY => {
                match wparam.0 as i32 {
                    HOTKEY_TOGGLE => toggle_enabled(),
                    HOTKEY_SPOT => toggle_spot(),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_TRAY => {
                let ev = (lparam.0 as u32) & 0xffff;
                if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU || ev == WM_LBUTTONUP {
                    show_tray_menu(hwnd);
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                match wparam.0 & 0xffff {
                    CMD_TOGGLE => toggle_enabled(),
                    CMD_SPOT => toggle_spot(),
                    CMD_SETTINGS => open_settings(),
                    CMD_EXIT => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                remove_tray(hwnd);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn toggle_enabled() {
    APP.with(|a| {
        if let Some(app) = a.borrow_mut().as_mut() {
            app.enabled = !app.enabled;
            if !app.enabled {
                app.ripples.clear();
                app.scrolls.clear();
                app.trail.clear();
                app.buttons_down = 0;
                app.keys.clear();
                unsafe {
                    let _ = ShowWindow(app.hwnd, SW_HIDE);
                    let _ = ShowWindow(app.keys_hwnd, SW_HIDE);
                }
                app.keys_visible = false;
            } else {
                app.cursor = cursor_pos();
                unsafe {
                    let _ = ShowWindow(app.hwnd, SW_SHOWNOACTIVATE);
                }
            }
            render_spot(app);
        }
    });
}

fn ripple_radius(cfg: &Config, age_ms: f32) -> f32 {
    let t = (age_ms / cfg.ripple_lifetime_ms).clamp(0.0, 1.0);
    let e = 1.0 - (1.0 - t) * (1.0 - t); // ease-out
    cfg.ripple_start_r + (cfg.ripple_end_r - cfg.ripple_start_r) * e
}

fn render(app: &mut App) {
    if !app.enabled {
        return;
    }
    {
        let now = Instant::now();
        app.last_render = now;
        let life = app.cfg.ripple_lifetime_ms;
        app.ripples
            .retain(|r| now.duration_since(r.start).as_secs_f32() * 1000.0 <= life);
        let slife = app.cfg.scroll_lifetime_ms;
        app.scrolls
            .retain(|s| now.duration_since(s.start).as_secs_f32() * 1000.0 <= slife);
        let dlife = app.cfg.drag_lifetime_ms;
        app.trail
            .retain(|d| now.duration_since(d.start).as_secs_f32() * 1000.0 <= dlife);

        // ---- bounding box of all active effects (virtual-screen coords) ----
        let mut minx = f32::MAX;
        let mut miny = f32::MAX;
        let mut maxx = f32::MIN;
        let mut maxy = f32::MIN;
        let pad = 3.0;

        let (cx, cy) = (app.cursor.0 as f32, app.cursor.1 as f32);
        let hr = app.cfg.highlight_radius + app.cfg.ring_thickness + pad;
        minx = minx.min(cx - hr);
        miny = miny.min(cy - hr);
        maxx = maxx.max(cx + hr);
        maxy = maxy.max(cy + hr);

        for r in &app.ripples {
            let age = now.duration_since(r.start).as_secs_f32() * 1000.0;
            let rad = ripple_radius(&app.cfg, age) + app.cfg.ripple_thickness + pad;
            minx = minx.min(r.x as f32 - rad);
            miny = miny.min(r.y as f32 - rad);
            maxx = maxx.max(r.x as f32 + rad);
            maxy = maxy.max(r.y as f32 + rad);
        }
        let dr = app.cfg.drag_dot_radius + pad;
        for d in &app.trail {
            minx = minx.min(d.x as f32 - dr);
            miny = miny.min(d.y as f32 - dr);
            maxx = maxx.max(d.x as f32 + dr);
            maxy = maxy.max(d.y as f32 + dr);
        }
        let sr = 22.0; // scroll indicator extent
        for s in &app.scrolls {
            minx = minx.min(s.x as f32 - sr);
            miny = miny.min(s.y as f32 - sr - 18.0);
            maxx = maxx.max(s.x as f32 + sr);
            maxy = maxy.max(s.y as f32 + sr + 18.0);
        }

        // clamp to virtual screen
        let (vl, vt, vr, vb) = app.vs;
        let bx = (minx.floor() as i32).max(vl);
        let by = (miny.floor() as i32).max(vt);
        let bxr = (maxx.ceil() as i32).min(vr);
        let byb = (maxy.ceil() as i32).min(vb);
        let w = bxr - bx;
        let h = byb - by;
        if w <= 0 || h <= 0 {
            return;
        }

        // ---- draw into premultiplied BGRA buffer ----
        let mut buf = vec![0u32; (w * h) as usize];
        let ox = bx as f32;
        let oy = by as f32;

        // cursor highlight: soft fill + ring
        let cfg = &app.cfg;
        draw_disc(&mut buf, w, h, cx - ox, cy - oy, cfg.highlight_radius, cfg.highlight_fill);
        draw_ring(
            &mut buf,
            w,
            h,
            cx - ox,
            cy - oy,
            cfg.highlight_radius,
            cfg.ring_thickness,
            cfg.highlight_ring,
        );

        // drag trail (drawn under ripples)
        for d in &app.trail {
            let age = now.duration_since(d.start).as_secs_f32() * 1000.0;
            let t = (age / cfg.drag_lifetime_ms).clamp(0.0, 1.0);
            let (cr, cg, cb, ca) = cfg.drag_color;
            let col = (cr, cg, cb, (ca as f32 * (1.0 - t)) as u8);
            draw_disc(&mut buf, w, h, d.x as f32 - ox, d.y as f32 - oy, cfg.drag_dot_radius, col);
        }

        // click ripples
        for r in &app.ripples {
            let age = now.duration_since(r.start).as_secs_f32() * 1000.0;
            let t = (age / cfg.ripple_lifetime_ms).clamp(0.0, 1.0);
            let rad = ripple_radius(cfg, age);
            let (cr, cg, cb, ca) = r.color;
            let a = (ca as f32 * (1.0 - t)) as u8;
            draw_ring(&mut buf, w, h, r.x as f32 - ox, r.y as f32 - oy, rad, cfg.ripple_thickness, (cr, cg, cb, a));
            if r.double {
                // inner ring for double-click emphasis
                draw_ring(&mut buf, w, h, r.x as f32 - ox, r.y as f32 - oy, rad * 0.6, cfg.ripple_thickness, (cr, cg, cb, a));
            }
        }

        // scroll indicators (filled triangle, drifts in scroll direction)
        for s in &app.scrolls {
            let age = now.duration_since(s.start).as_secs_f32() * 1000.0;
            let t = (age / cfg.scroll_lifetime_ms).clamp(0.0, 1.0);
            let (cr, cg, cb, ca) = cfg.scroll_color;
            let a = (ca as f32 * (1.0 - t)) as u8;
            let drift = 14.0 * t;
            let cxs = s.x as f32 - ox + 26.0;
            let cys = s.y as f32 - oy + if s.up { -drift } else { drift };
            let hw = 9.0;
            let hh = 11.0;
            let (p0, p1, p2) = if s.up {
                ((cxs, cys - hh), (cxs - hw, cys + hh), (cxs + hw, cys + hh))
            } else {
                ((cxs, cys + hh), (cxs - hw, cys - hh), (cxs + hw, cys - hh))
            };
            draw_triangle(&mut buf, w, h, p0, p1, p2, (cr, cg, cb, a));
        }

        present(app.hwnd, bx, by, w, h, &buf);
    }
}

fn render_spot(app: &mut App) {
    let on = app.enabled && app.cfg.spotlight_enabled;
    if !on {
        if app.spot_visible {
            unsafe {
                let _ = ShowWindow(app.spot_hwnd, SW_HIDE);
            }
            app.spot_visible = false;
        }
        return;
    }

    // skip rebuild when nothing changed (avoid full-screen redraw at idle);
    // keep refreshing while the settings window is open for live preview.
    if app.spot_visible && app.cursor == app.spot_last && app.settings_hwnd.0.is_null() {
        return;
    }
    app.spot_last = app.cursor;

    let (vl, vt, vr, vb) = app.vs;
    let w = vr - vl;
    let h = vb - vt;
    if w <= 0 || h <= 0 {
        return;
    }
    let n = (w * h) as usize;
    if app.spot_buf.len() != n {
        app.spot_buf = vec![0u32; n];
    }

    let dim = app.cfg.spotlight_dim as f32;
    let veil = (app.cfg.spotlight_dim as u32) << 24; // premultiplied black, alpha=dim
    app.spot_buf.fill(veil);

    // carve the bright hole around the cursor
    let cx = (app.cursor.0 - vl) as f32;
    let cy = (app.cursor.1 - vt) as f32;
    let r = app.cfg.spotlight_radius;
    let f = app.cfg.spotlight_feather.max(0.5);
    let rmax = r + f;
    let x0 = ((cx - rmax).floor() as i32).max(0);
    let y0 = ((cy - rmax).floor() as i32).max(0);
    let x1 = ((cx + rmax).ceil() as i32).min(w);
    let y1 = ((cy + rmax).ceil() as i32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            if d < rmax {
                let t = ((d - r) / f).clamp(0.0, 1.0); // 0 inside hole -> 1 at veil
                let a = (dim * t) as u32;
                app.spot_buf[(y * w + x) as usize] = a << 24;
            }
        }
    }

    unsafe {
        if !app.spot_visible {
            let _ = ShowWindow(app.spot_hwnd, SW_SHOWNOACTIVATE);
            app.spot_visible = true;
            // keep the effect windows above the veil so they aren't dimmed
            let _ = SetWindowPos(app.keys_hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let _ = SetWindowPos(app.hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
    present(app.spot_hwnd, vl, vt, w, h, &app.spot_buf);
}

fn toggle_spot() {
    APP.with(|a| {
        if let Some(app) = a.borrow_mut().as_mut() {
            app.cfg.spotlight_enabled = !app.cfg.spotlight_enabled;
            app.cfg.save();
            render_spot(app);
            if !app.settings_hwnd.0.is_null() {
                unsafe {
                    if let Ok(cb) = GetDlgItem(app.settings_hwnd, ID_SPOT_ON) {
                        let c = if app.cfg.spotlight_enabled { 1 } else { 0 };
                        let _ = SendMessageW(cb, BM_SETCHECK, WPARAM(c), LPARAM(0));
                    }
                }
            }
        }
    });
}

/// Blend a straight-alpha color over a premultiplied BGRA pixel.
#[inline]
fn blend(dst: &mut u32, r: u8, g: u8, b: u8, a: u16) {
    if a == 0 {
        return;
    }
    let a = a.min(255);
    let inv = 255 - a;
    let d = *dst;
    let da = (d >> 24) & 0xff;
    let dr = (d >> 16) & 0xff;
    let dg = (d >> 8) & 0xff;
    let db = d & 0xff;
    let nr = ((r as u16 * a) / 255 + (dr as u16 * inv) / 255).min(255);
    let ng = ((g as u16 * a) / 255 + (dg as u16 * inv) / 255).min(255);
    let nb = ((b as u16 * a) / 255 + (db as u16 * inv) / 255).min(255);
    let na = (a + (da as u16 * inv) / 255).min(255);
    *dst = ((na as u32) << 24) | ((nr as u32) << 16) | ((ng as u32) << 8) | nb as u32;
}

fn draw_disc(buf: &mut [u32], w: i32, h: i32, cx: f32, cy: f32, radius: f32, color: Rgba) {
    let (r, g, b, base_a) = color;
    let rmax = radius + 1.0;
    let x0 = ((cx - rmax).floor() as i32).max(0);
    let y0 = ((cy - rmax).floor() as i32).max(0);
    let x1 = ((cx + rmax).ceil() as i32).min(w);
    let y1 = ((cy + rmax).ceil() as i32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let cov = (radius - d + 0.5).clamp(0.0, 1.0);
            if cov > 0.0 {
                let a = (base_a as f32 * cov) as u16;
                blend(&mut buf[(y * w + x) as usize], r, g, b, a);
            }
        }
    }
}

fn draw_ring(buf: &mut [u32], w: i32, h: i32, cx: f32, cy: f32, radius: f32, thickness: f32, color: Rgba) {
    let (r, g, b, base_a) = color;
    let half = thickness * 0.5;
    let rmax = radius + half + 1.0;
    let x0 = ((cx - rmax).floor() as i32).max(0);
    let y0 = ((cy - rmax).floor() as i32).max(0);
    let x1 = ((cx + rmax).ceil() as i32).min(w);
    let y1 = ((cy + rmax).ceil() as i32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let cov = (half - (d - radius).abs() + 0.5).clamp(0.0, 1.0);
            if cov > 0.0 {
                let a = (base_a as f32 * cov) as u16;
                blend(&mut buf[(y * w + x) as usize], r, g, b, a);
            }
        }
    }
}

fn draw_triangle(buf: &mut [u32], w: i32, h: i32, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), color: Rgba) {
    let (r, g, b, base_a) = color;
    let edge = |a: (f32, f32), b: (f32, f32), px: f32, py: f32| (px - a.0) * (b.1 - a.1) - (py - a.1) * (b.0 - a.0);
    let len = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let s = if edge(p0, p1, p2.0, p2.1) >= 0.0 { 1.0 } else { -1.0 };
    let (l0, l1, l2) = (len(p0, p1), len(p1, p2), len(p2, p0));
    let x0 = (p0.0.min(p1.0).min(p2.0).floor() as i32 - 1).max(0);
    let y0 = (p0.1.min(p1.1).min(p2.1).floor() as i32 - 1).max(0);
    let x1 = (p0.0.max(p1.0).max(p2.0).ceil() as i32 + 1).min(w);
    let y1 = (p0.1.max(p1.1).max(p2.1).ceil() as i32 + 1).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let d0 = s * edge(p0, p1, px, py) / l0;
            let d1 = s * edge(p1, p2, px, py) / l1;
            let d2 = s * edge(p2, p0, px, py) / l2;
            let cov = (0.5 + d0.min(d1).min(d2)).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend(&mut buf[(y * w + x) as usize], r, g, b, (base_a as f32 * cov) as u16);
            }
        }
    }
}

fn present(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, buf: &[u32]) {
    unsafe {
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(screen_dc);

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        if let Ok(dib) = dib {
            if !bits.is_null() {
                std::ptr::copy_nonoverlapping(buf.as_ptr(), bits as *mut u32, buf.len());
            }
            let old = SelectObject(mem_dc, HGDIOBJ(dib.0));

            let pt_dst = POINT { x, y };
            let sz = SIZE { cx: w, cy: h };
            let pt_src = POINT { x: 0, y: 0 };
            let blend_fn = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                screen_dc,
                Some(&pt_dst),
                Some(&sz),
                mem_dc,
                Some(&pt_src),
                COLORREF(0),
                Some(&blend_fn),
                ULW_ALPHA,
            );

            SelectObject(mem_dc, old);
            let _ = DeleteObject(HGDIOBJ(dib.0));
        }
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);
    }
}

// ---------------- key-stroke display ----------------

fn key_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 }
}

fn is_modifier(vk: u32) -> bool {
    let v = vk as u16;
    [
        VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_MENU, VK_LMENU,
        VK_RMENU, VK_LWIN, VK_RWIN,
    ]
    .iter()
    .any(|m| m.0 == v)
}

fn key_name(vk: u32) -> Option<String> {
    // letters / digits
    if (0x41..=0x5A).contains(&vk) || (0x30..=0x39).contains(&vk) {
        return Some((vk as u8 as char).to_string());
    }
    if (VK_NUMPAD0.0 as u32..=VK_NUMPAD9.0 as u32).contains(&vk) {
        return Some(format!("Num{}", vk - VK_NUMPAD0.0 as u32));
    }
    if (VK_F1.0 as u32..=VK_F24.0 as u32).contains(&vk) {
        return Some(format!("F{}", vk - VK_F1.0 as u32 + 1));
    }
    let v = vk as u16;
    let named = if v == VK_SPACE.0 {
        "Space"
    } else if v == VK_RETURN.0 {
        "Enter"
    } else if v == VK_ESCAPE.0 {
        "Esc"
    } else if v == VK_TAB.0 {
        "Tab"
    } else if v == VK_BACK.0 {
        "Backspace"
    } else if v == VK_DELETE.0 {
        "Delete"
    } else if v == VK_INSERT.0 {
        "Insert"
    } else if v == VK_HOME.0 {
        "Home"
    } else if v == VK_END.0 {
        "End"
    } else if v == VK_PRIOR.0 {
        "PageUp"
    } else if v == VK_NEXT.0 {
        "PageDown"
    } else if v == VK_LEFT.0 {
        "\u{2190}"
    } else if v == VK_UP.0 {
        "\u{2191}"
    } else if v == VK_RIGHT.0 {
        "\u{2192}"
    } else if v == VK_DOWN.0 {
        "\u{2193}"
    } else if v == VK_SNAPSHOT.0 {
        "PrtSc"
    } else {
        ""
    };
    if !named.is_empty() {
        return Some(named.to_string());
    }
    // fallback: printable character for OEM/punctuation keys
    let ch = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) } & 0x7fff;
    if ch >= 0x20 {
        if let Some(c) = char::from_u32(ch) {
            return Some(c.to_string());
        }
    }
    None
}

extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let vk = kb.vkCode;
            APP.with(|a| {
                if let Some(app) = a.borrow_mut().as_mut() {
                    if app.enabled && !is_modifier(vk) {
                        if let Some(name) = key_name(vk) {
                            let mut parts: Vec<String> = Vec::new();
                            if key_down(VK_CONTROL) {
                                parts.push("Ctrl".into());
                            }
                            if key_down(VK_LWIN) || key_down(VK_RWIN) {
                                parts.push("Win".into());
                            }
                            if key_down(VK_MENU) {
                                parts.push("Alt".into());
                            }
                            if key_down(VK_SHIFT) {
                                parts.push("Shift".into());
                            }
                            if app.cfg.show_all_keys || !parts.is_empty() {
                                parts.push(name);
                                let entry = KeyEntry {
                                    text: parts.join(" + "),
                                    shown_at: Instant::now(),
                                    mask: None,
                                };
                                if !app.cfg.key_stack {
                                    app.keys.clear(); // 上書き型
                                }
                                app.keys.push(entry);
                                let max = app.cfg.key_max.max(1);
                                if app.keys.len() > max {
                                    let drop = app.keys.len() - max;
                                    app.keys.drain(0..drop);
                                }
                                render_keys(app);
                            }
                        }
                    }
                }
            });
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Rasterise text to an 8-bit coverage mask via GDI (grayscale-AA white on black).
fn text_mask(text: &str, font_px: i32) -> (i32, i32, Vec<u8>) {
    unsafe {
        let dc = CreateCompatibleDC(HDC(std::ptr::null_mut()));

        let mut lf = LOGFONTW {
            lfHeight: -font_px,
            lfWeight: 700,
            lfQuality: ANTIALIASED_QUALITY,
            ..Default::default()
        };
        for (i, c) in "Segoe UI".encode_utf16().enumerate() {
            lf.lfFaceName[i] = c;
        }
        let font = CreateFontIndirectW(&lf);
        let old_font = SelectObject(dc, HGDIOBJ(font.0));

        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut sz = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &wide, &mut sz);
        let tw = sz.cx + 2;
        let th = sz.cy + 2;
        if tw <= 0 || th <= 0 {
            SelectObject(dc, old_font);
            let _ = DeleteObject(HGDIOBJ(font.0));
            let _ = DeleteDC(dc);
            return (0, 0, Vec::new());
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: tw,
                biHeight: -th,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0);

        let mut mask = vec![0u8; (tw * th) as usize];
        if let Ok(dib) = dib {
            let old_bmp = SelectObject(dc, HGDIOBJ(dib.0));
            let _ = SetBkMode(dc, TRANSPARENT);
            let _ = SetTextColor(dc, COLORREF(0x00FF_FFFF));
            let _ = TextOutW(dc, 1, 1, &wide);
            if !bits.is_null() {
                let px = std::slice::from_raw_parts(bits as *const u32, (tw * th) as usize);
                for (i, p) in px.iter().enumerate() {
                    mask[i] = (p & 0xff) as u8; // grayscale coverage
                }
            }
            SelectObject(dc, old_bmp);
            let _ = DeleteObject(HGDIOBJ(dib.0));
        }

        SelectObject(dc, old_font);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteDC(dc);
        (tw, th, mask)
    }
}

/// Filled rounded rectangle occupying [x0, x0+w) x [y0, y0+h) inside `buf`.
fn fill_round_rect(buf: &mut [u32], buf_w: i32, x0: i32, y0: i32, w: i32, h: i32, radius: f32, color: Rgba) {
    let (r, g, b, base_a) = color;
    let hw = w as f32 / 2.0;
    let hh = h as f32 / 2.0;
    let rad = radius.min(hw).min(hh);
    for yy in 0..h {
        for xx in 0..w {
            let px = (xx as f32 + 0.5) - hw;
            let py = (yy as f32 + 0.5) - hh;
            let qx = px.abs() - (hw - rad);
            let qy = py.abs() - (hh - rad);
            let ox = qx.max(0.0);
            let oy = qy.max(0.0);
            let d = (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - rad;
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend(&mut buf[((y0 + yy) * buf_w + (x0 + xx)) as usize], r, g, b, (base_a as f32 * cov) as u16);
            }
        }
    }
}

struct Pill {
    tw: i32,
    th: i32,
    mask: Vec<u8>,
    w: i32,
    h: i32,
    fade: f32,
}

fn render_keys(app: &mut App) {
    // drop expired entries
    let now = Instant::now();
    let life = app.cfg.key_lifetime_ms;
    app.keys
        .retain(|k| now.duration_since(k.shown_at).as_secs_f32() * 1000.0 <= life);

    if !app.enabled || app.keys.is_empty() {
        if app.keys_visible {
            unsafe {
                let _ = ShowWindow(app.keys_hwnd, SW_HIDE);
            }
            app.keys_visible = false;
        }
        return;
    }

    // snapshot config (Copy) so we can mutate `app` later without borrow conflicts
    let (padx, pady) = app.cfg.key_padding;
    let gap = app.cfg.key_gap;
    let radius = app.cfg.key_radius;
    let font_px = app.cfg.key_font_px;
    let fade_ms = app.cfg.key_fade_ms;
    let bg = app.cfg.key_bg;
    let tc = app.cfg.key_text_color;
    let margin = app.cfg.key_margin_bottom;
    let key_pos = app.cfg.key_pos;
    let key_x = app.cfg.key_x;
    let key_y = app.cfg.key_y;

    // rasterise text once per entry (cached); fade changes per frame, mask does not
    for k in app.keys.iter_mut() {
        if k.mask.is_none() {
            k.mask = Some(text_mask(&k.text, font_px));
        }
    }

    // build one pill per visible key
    let mut pills: Vec<Pill> = Vec::new();
    for k in &app.keys {
        let elapsed = now.duration_since(k.shown_at).as_secs_f32() * 1000.0;
        let fade = if elapsed > life - fade_ms {
            ((life - elapsed) / fade_ms).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let (tw, th, mask) = match &k.mask {
            Some(m) if m.0 > 0 && m.1 > 0 => m,
            _ => continue,
        };
        pills.push(Pill {
            tw: *tw,
            th: *th,
            mask: mask.clone(),
            w: *tw + padx * 2,
            h: *th + pady * 2,
            fade,
        });
    }
    if pills.is_empty() {
        return;
    }

    // Drop oldest keys (from the left) when the row would overflow the screen width.
    let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let avail = (sw - 48).max(100); // leave a small side margin
    let mut drop_n = 0usize;
    while pills.len() - drop_n > 1 {
        let n = (pills.len() - drop_n) as i32;
        let w: i32 = pills[drop_n..].iter().map(|p| p.w).sum::<i32>() + gap * (n - 1);
        if w <= avail {
            break;
        }
        drop_n += 1;
    }
    if drop_n > 0 {
        pills.drain(0..drop_n);
        let d = drop_n.min(app.keys.len());
        app.keys.drain(0..d); // keep state in sync so they stay gone next frame
    }

    let total_w: i32 = pills.iter().map(|p| p.w).sum::<i32>() + gap * (pills.len() as i32 - 1);
    let max_h: i32 = pills.iter().map(|p| p.h).max().unwrap_or(0);
    let mut buf = vec![0u32; (total_w * max_h) as usize];

    let (br, bgc, bb, ba) = bg;
    let (tr, tg, tb, ta) = tc;
    let mut x = 0;
    for p in &pills {
        let y0 = max_h - p.h; // bottom-align
        fill_round_rect(&mut buf, total_w, x, y0, p.w, p.h, radius, (br, bgc, bb, (ba as f32 * p.fade) as u8));
        for ty in 0..p.th {
            for tx in 0..p.tw {
                let cov = p.mask[(ty * p.tw + tx) as usize];
                if cov > 0 {
                    let a = (cov as f32 * (ta as f32 / 255.0) * p.fade) as u16;
                    let bx = x + padx + tx;
                    let by = y0 + pady + ty;
                    blend(&mut buf[(by * total_w + bx) as usize], tr, tg, tb, a);
                }
            }
        }
        x += p.w + gap;
    }

    unsafe {
        let side = 24;
        let cxp = (sw - total_w) / 2;
        let topy = margin;
        let boty = sh - margin - max_h;
        let (px, py) = match key_pos {
            1 => (side, boty),                  // 下左
            2 => (sw - total_w - side, boty),   // 下右
            3 => (cxp, topy),                   // 上中央
            4 => (side, topy),                  // 上左
            5 => (sw - total_w - side, topy),   // 上右
            6 => (cxp, (sh - max_h) / 2),       // 中央
            7 => clamp_pos(key_x, key_y, total_w, max_h), // 座標指定
            _ => (cxp, boty),                   // 下中央 (既定)
        };
        if !app.keys_visible {
            let _ = ShowWindow(app.keys_hwnd, SW_SHOWNOACTIVATE);
            app.keys_visible = true;
        }
        present(app.keys_hwnd, px, py, total_w, max_h, &buf);
    }
}

// ---------------- settings window ----------------

const VAL_OFFSET: i32 = 0x4000;

fn makelong(lo: i32, hi: i32) -> isize {
    ((lo as u16 as u32) | ((hi as u16 as u32) << 16)) as isize
}

fn is_color_id(id: i32) -> bool {
    COLORS.iter().any(|(cid, _, _)| *cid == id)
}

fn get_slider(cfg: &Config, id: i32) -> i32 {
    match id {
        ID_HL_RADIUS => cfg.highlight_radius as i32,
        ID_HL_FILL_A => cfg.highlight_fill.3 as i32,
        ID_RING_A => cfg.highlight_ring.3 as i32,
        ID_RIPPLE_R => cfg.ripple_end_r as i32,
        ID_RIPPLE_MS => cfg.ripple_lifetime_ms as i32,
        ID_KEY_FONT => cfg.key_font_px,
        ID_KEY_MS => cfg.key_lifetime_ms as i32,
        ID_KEY_GAP => cfg.key_gap,
        ID_DRAG_R => cfg.drag_dot_radius as i32,
        ID_DRAG_MS => cfg.drag_lifetime_ms as i32,
        ID_SCROLL_MS => cfg.scroll_lifetime_ms as i32,
        ID_KEY_X => cfg.key_x,
        ID_KEY_Y => cfg.key_y,
        ID_SPOT_RADIUS => cfg.spotlight_radius as i32,
        ID_SPOT_DIM => cfg.spotlight_dim as i32,
        ID_SPOT_FEATHER => cfg.spotlight_feather as i32,
        _ => 0,
    }
}

fn set_slider(cfg: &mut Config, id: i32, v: i32) {
    match id {
        ID_HL_RADIUS => cfg.highlight_radius = v as f32,
        ID_HL_FILL_A => cfg.highlight_fill.3 = v as u8,
        ID_RING_A => cfg.highlight_ring.3 = v as u8,
        ID_RIPPLE_R => cfg.ripple_end_r = v as f32,
        ID_RIPPLE_MS => cfg.ripple_lifetime_ms = v as f32,
        ID_KEY_FONT => cfg.key_font_px = v,
        ID_KEY_MS => cfg.key_lifetime_ms = v as f32,
        ID_KEY_GAP => cfg.key_gap = v,
        ID_DRAG_R => cfg.drag_dot_radius = v as f32,
        ID_DRAG_MS => cfg.drag_lifetime_ms = v as f32,
        ID_SCROLL_MS => cfg.scroll_lifetime_ms = v as f32,
        ID_KEY_X => cfg.key_x = v,
        ID_KEY_Y => cfg.key_y = v,
        ID_SPOT_RADIUS => cfg.spotlight_radius = v as f32,
        ID_SPOT_DIM => cfg.spotlight_dim = v as u8,
        ID_SPOT_FEATHER => cfg.spotlight_feather = v as f32,
        _ => {}
    }
}

fn get_color(cfg: &Config, id: i32) -> Rgba {
    match id {
        ID_COL_FILL => cfg.highlight_fill,
        ID_COL_RING => cfg.highlight_ring,
        ID_COL_LEFT => cfg.left_color,
        ID_COL_RIGHT => cfg.right_color,
        ID_COL_MIDDLE => cfg.middle_color,
        ID_COL_DOUBLE => cfg.double_color,
        ID_COL_SCROLL => cfg.scroll_color,
        ID_COL_DRAG => cfg.drag_color,
        ID_COL_KEYBG => cfg.key_bg,
        ID_COL_KEYTX => cfg.key_text_color,
        _ => (0, 0, 0, 255),
    }
}

fn set_color(cfg: &mut Config, id: i32, rgb: (u8, u8, u8)) {
    let (r, g, b) = rgb;
    let f = |a: u8| (r, g, b, a);
    match id {
        ID_COL_FILL => cfg.highlight_fill = f(cfg.highlight_fill.3),
        ID_COL_RING => cfg.highlight_ring = f(cfg.highlight_ring.3),
        ID_COL_LEFT => cfg.left_color = f(cfg.left_color.3),
        ID_COL_RIGHT => cfg.right_color = f(cfg.right_color.3),
        ID_COL_MIDDLE => cfg.middle_color = f(cfg.middle_color.3),
        ID_COL_DOUBLE => cfg.double_color = f(cfg.double_color.3),
        ID_COL_SCROLL => cfg.scroll_color = f(cfg.scroll_color.3),
        ID_COL_DRAG => cfg.drag_color = f(cfg.drag_color.3),
        ID_COL_KEYBG => cfg.key_bg = f(cfg.key_bg.3),
        ID_COL_KEYTX => cfg.key_text_color = f(cfg.key_text_color.3),
        _ => {}
    }
}

const CHECKS: &[(i32, &str, i32)] = &[
    (ID_SHOW_DRAG, "ドラッグ軌跡を表示", 0),
    (ID_SHOW_SCROLL, "スクロール方向を表示", 0),
    (ID_SPOT_ON, "スポットライト：周囲を暗く (Ctrl+Alt+S)", 0),
    (ID_KEY_STACK, "キー: 追加型で表示 (オフ=上書き型)", 1),
    (ID_SHOW_ALL, "全てのキーを表示 (オフ=ショートカットのみ)", 1),
];

fn is_check(cfg: &Config, id: i32) -> bool {
    match id {
        ID_KEY_STACK => cfg.key_stack,
        ID_SHOW_ALL => cfg.show_all_keys,
        ID_SHOW_DRAG => cfg.show_drag,
        ID_SHOW_SCROLL => cfg.show_scroll,
        ID_SPOT_ON => cfg.spotlight_enabled,
        _ => false,
    }
}

fn set_check(cfg: &mut Config, id: i32, v: bool) {
    match id {
        ID_KEY_STACK => cfg.key_stack = v,
        ID_SHOW_ALL => cfg.show_all_keys = v,
        ID_SHOW_DRAG => cfg.show_drag = v,
        ID_SHOW_SCROLL => cfg.show_scroll = v,
        ID_SPOT_ON => cfg.spotlight_enabled = v,
        _ => {}
    }
}

fn is_check_id(id: i32) -> bool {
    CHECKS.iter().any(|(cid, _, _)| *cid == id)
}

unsafe fn mk(parent: HWND, class: PCWSTR, text: &str, style: WINDOW_STYLE, x: i32, y: i32, w: i32, h: i32, id: i32, hinst: HINSTANCE) -> HWND {
    let t = HSTRING::from(text);
    let menu = HMENU(id as usize as *mut core::ffi::c_void);
    let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), class, &t, style, x, y, w, h, parent, menu, hinst, None)
        .unwrap_or(HWND(std::ptr::null_mut()));
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let _ = SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
    hwnd
}

fn refresh_controls(hwnd: HWND, cfg: &Config) {
    unsafe {
        for (id, _, _, _, _) in SLIDERS {
            if let Ok(tb) = GetDlgItem(hwnd, *id) {
                let v = get_slider(cfg, *id);
                let _ = SendMessageW(tb, TBM_SETPOS, WPARAM(1), LPARAM(v as isize));
                if let Ok(vl) = GetDlgItem(hwnd, *id + VAL_OFFSET) {
                    let _ = SetWindowTextW(vl, &HSTRING::from(v.to_string()));
                }
            }
        }
        for (id, _, _) in CHECKS {
            if let Ok(cb) = GetDlgItem(hwnd, *id) {
                let c = if is_check(cfg, *id) { 1 } else { 0 };
                let _ = SendMessageW(cb, BM_SETCHECK, WPARAM(c), LPARAM(0));
            }
        }
        if let Ok(combo) = GetDlgItem(hwnd, ID_KEY_POS) {
            let _ = SendMessageW(combo, CB_SETCURSEL, WPARAM(cfg.key_pos.max(0) as usize), LPARAM(0));
        }
    }
}

fn select_page(app: &App, sel: usize) {
    unsafe {
        for (p, list) in app.settings_pages.iter().enumerate() {
            let cmd = if p == sel { SW_SHOW } else { SW_HIDE };
            for &h in list {
                let _ = ShowWindow(h, cmd);
            }
        }
    }
}

fn open_settings() {
    APP.with(|a| {
        let mut b = a.borrow_mut();
        let app = match b.as_mut() {
            Some(x) => x,
            None => return,
        };
        if !app.settings_hwnd.0.is_null() {
            unsafe {
                let _ = ShowWindow(app.settings_hwnd, SW_SHOW);
                let _ = SetForegroundWindow(app.settings_hwnd);
            }
            return;
        }
        let hinst = app.hinstance;
        let win_style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("MouseSpotSettings"),
                w!("MouseSpot 設定"),
                win_style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                480,
                560,
                None,
                None,
                hinst,
                None,
            )
            .unwrap_or(HWND(std::ptr::null_mut()));
            if hwnd.0.is_null() {
                return;
            }
            app.settings_hwnd = hwnd;

            let cs = WS_CHILD.0 | WS_VISIBLE.0;
            let st_static = WINDOW_STYLE(cs);
            let st_track = WINDOW_STYLE(cs | WS_TABSTOP.0 | 0x0010); // TBS_NOTICKS
            let st_check = WINDOW_STYLE(cs | WS_TABSTOP.0 | 0x0003); // BS_AUTOCHECKBOX
            let st_btn = WINDOW_STYLE(cs | WS_TABSTOP.0);

            // tab control
            let tab = mk(hwnd, w!("SysTabControl32"), "", WINDOW_STYLE(cs | WS_TABSTOP.0), 8, 8, 456, 30, ID_TAB, hinst);
            for (i, name) in ["マウス操作", "キー操作"].iter().enumerate() {
                let s = HSTRING::from(*name);
                let mut it = TcItem {
                    mask: TCIF_TEXT,
                    dw_state: 0,
                    dw_state_mask: 0,
                    psz_text: s.as_ptr() as *mut u16,
                    cch_text_max: 0,
                    i_image: -1,
                    l_param: 0,
                };
                let _ = SendMessageW(tab, TCM_INSERTITEMW, WPARAM(i), LPARAM(&mut it as *mut TcItem as isize));
            }

            let cfg = app.cfg.clone();
            let content_top = 50;
            let mut max_bottom = content_top;

            for page in 0..2usize {
                let mut y = content_top;

                for (id, label, min, max, pg) in SLIDERS {
                    if *pg as usize != page {
                        continue;
                    }
                    let lab = mk(hwnd, w!("STATIC"), label, st_static, 16, y + 4, 180, 20, 0, hinst);
                    let tb = mk(hwnd, w!("msctls_trackbar32"), "", st_track, 200, y, 210, 28, *id, hinst);
                    let _ = SendMessageW(tb, TBM_SETRANGE, WPARAM(1), LPARAM(makelong(*min, *max)));
                    let v = get_slider(&cfg, *id);
                    let _ = SendMessageW(tb, TBM_SETPOS, WPARAM(1), LPARAM(v as isize));
                    let val = mk(hwnd, w!("STATIC"), &v.to_string(), st_static, 418, y + 4, 48, 20, *id + VAL_OFFSET, hinst);
                    app.settings_pages[page].extend([lab, tb, val]);
                    y += 34;
                }

                // key position combobox (key page only)
                if page == 1 {
                    y += 4;
                    let lab = mk(hwnd, w!("STATIC"), "キー表示位置", st_static, 16, y + 4, 110, 20, 0, hinst);
                    let st_combo = WINDOW_STYLE(cs | WS_TABSTOP.0 | WS_VSCROLL.0 | 0x0003); // CBS_DROPDOWNLIST
                    let combo = mk(hwnd, w!("COMBOBOX"), "", st_combo, 130, y, 200, 240, ID_KEY_POS, hinst);
                    for item in ["下中央", "下左", "下右", "上中央", "上左", "上右", "中央", "座標指定(X/Y)"] {
                        let s = HSTRING::from(item);
                        let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(s.as_ptr() as isize));
                    }
                    let _ = SendMessageW(combo, CB_SETCURSEL, WPARAM(cfg.key_pos.max(0) as usize), LPARAM(0));
                    app.settings_pages[page].extend([lab, combo]);
                    y += 38;
                }

                y += 4;
                for (id, label, pg) in CHECKS {
                    if *pg as usize != page {
                        continue;
                    }
                    let cb = mk(hwnd, w!("BUTTON"), label, st_check, 16, y, 440, 22, *id, hinst);
                    let c = if is_check(&cfg, *id) { 1 } else { 0 };
                    let _ = SendMessageW(cb, BM_SETCHECK, WPARAM(c), LPARAM(0));
                    app.settings_pages[page].push(cb);
                    y += 28;
                }

                y += 6;
                let page_cols: Vec<&(i32, &str, i32)> = COLORS.iter().filter(|(_, _, pg)| *pg as usize == page).collect();
                let mut i = 0;
                while i < page_cols.len() {
                    let (id, label, _) = page_cols[i];
                    let b1 = mk(hwnd, w!("BUTTON"), label, st_btn, 16, y, 210, 26, *id, hinst);
                    app.settings_pages[page].push(b1);
                    if i + 1 < page_cols.len() {
                        let (id2, label2, _) = page_cols[i + 1];
                        let b2 = mk(hwnd, w!("BUTTON"), label2, st_btn, 246, y, 210, 26, *id2, hinst);
                        app.settings_pages[page].push(b2);
                    }
                    y += 32;
                    i += 2;
                }

                if y > max_bottom {
                    max_bottom = y;
                }
            }

            // bottom buttons (shared across pages)
            let mut y = max_bottom + 12;
            mk(hwnd, w!("BUTTON"), "既定に戻す", st_btn, 16, y, 150, 32, ID_RESET, hinst);
            mk(hwnd, w!("BUTTON"), "閉じる", st_btn, 306, y, 150, 32, ID_CLOSE, hinst);
            y += 48;

            // show page 0, hide page 1
            select_page(app, 0);

            // size to fit content
            let mut rc = RECT { left: 0, top: 0, right: 472, bottom: y };
            let _ = AdjustWindowRect(&mut rc, win_style, false);
            let outer_w = rc.right - rc.left;
            let outer_h = rc.bottom - rc.top;

            // position: remembered location, else near the tray (bottom-right of work area)
            let (px, py) = if let Some((sx, sy)) = load_win_pos() {
                clamp_pos(sx, sy, outer_w, outer_h)
            } else {
                let mut wa = RECT::default();
                let _ = SystemParametersInfoW(
                    SPI_GETWORKAREA,
                    0,
                    Some(&mut wa as *mut RECT as *mut core::ffi::c_void),
                    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                );
                (wa.right - outer_w - 8, wa.bottom - outer_h - 8)
            };
            let _ = SetWindowPos(hwnd, None, px, py, outer_w, outer_h, SWP_NOZORDER);

            let _ = ShowWindow(hwnd, SW_SHOW);
        }
    });
}

fn choose_color(owner: HWND, cur: (u8, u8, u8)) -> Option<(u8, u8, u8)> {
    unsafe {
        let mut cust = [COLORREF(0); 16];
        let init = COLORREF((cur.0 as u32) | ((cur.1 as u32) << 8) | ((cur.2 as u32) << 16));
        let mut cc = CHOOSECOLORW {
            lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
            hwndOwner: owner,
            rgbResult: init,
            lpCustColors: cust.as_mut_ptr(),
            Flags: CC_RGBINIT | CC_FULLOPEN,
            ..Default::default()
        };
        if ChooseColorW(&mut cc).as_bool() {
            let cr = cc.rgbResult.0;
            Some(((cr & 0xff) as u8, ((cr >> 8) & 0xff) as u8, ((cr >> 16) & 0xff) as u8))
        } else {
            None
        }
    }
}

extern "system" fn settings_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_HSCROLL => {
                let tb = HWND(lparam.0 as *mut core::ffi::c_void);
                let id = GetDlgCtrlID(tb);
                let code = (wparam.0 & 0xffff) as u32;
                let pos = SendMessageW(tb, TBM_GETPOS, WPARAM(0), LPARAM(0)).0 as i32;
                APP.with(|a| {
                    if let Some(app) = a.borrow_mut().as_mut() {
                        set_slider(&mut app.cfg, id, pos);
                        // adjusting X/Y switches to coordinate mode and shows a live preview pill
                        if id == ID_KEY_X || id == ID_KEY_Y {
                            app.cfg.key_pos = 7;
                            app.keys.clear();
                            app.keys.push(KeyEntry {
                                text: "プレビュー".into(),
                                shown_at: Instant::now(),
                                mask: None,
                            });
                            render_keys(app);
                        }
                        // persist on release / discrete steps, not during continuous drag
                        if code != TB_THUMBTRACK {
                            app.cfg.save();
                        }
                    }
                });
                if let Ok(vl) = GetDlgItem(hwnd, id + VAL_OFFSET) {
                    let _ = SetWindowTextW(vl, &HSTRING::from(pos.to_string()));
                }
                if (id == ID_KEY_X || id == ID_KEY_Y) && code != TB_THUMBTRACK {
                    if let Ok(combo) = GetDlgItem(hwnd, ID_KEY_POS) {
                        let _ = SendMessageW(combo, CB_SETCURSEL, WPARAM(7), LPARAM(0));
                    }
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                let id = (wparam.0 & 0xffff) as i32;
                let code = ((wparam.0 >> 16) & 0xffff) as u32;
                if id == ID_KEY_POS && code == CBN_SELCHANGE {
                    if let Ok(combo) = GetDlgItem(hwnd, ID_KEY_POS) {
                        let sel = SendMessageW(combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as i32;
                        if sel >= 0 {
                            APP.with(|a| {
                                if let Some(app) = a.borrow_mut().as_mut() {
                                    app.cfg.key_pos = sel;
                                    app.cfg.save();
                                    // preview so the chosen position is visible immediately
                                    app.keys.clear();
                                    app.keys.push(KeyEntry {
                                        text: "プレビュー".into(),
                                        shown_at: Instant::now(),
                                        mask: None,
                                    });
                                    render_keys(app);
                                }
                            });
                        }
                    }
                } else if id == ID_CLOSE {
                    let _ = DestroyWindow(hwnd);
                } else if id == ID_RESET {
                    APP.with(|a| {
                        if let Some(app) = a.borrow_mut().as_mut() {
                            app.cfg = Config::default();
                            app.cfg.save();
                        }
                    });
                    let cfg = APP.with(|a| a.borrow().as_ref().map(|x| x.cfg.clone()));
                    if let Some(cfg) = cfg {
                        refresh_controls(hwnd, &cfg);
                    }
                } else if is_check_id(id) {
                    if let Ok(cb) = GetDlgItem(hwnd, id) {
                        let checked = SendMessageW(cb, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1;
                        APP.with(|a| {
                            if let Some(app) = a.borrow_mut().as_mut() {
                                set_check(&mut app.cfg, id, checked);
                                app.cfg.save();
                            }
                        });
                    }
                } else if is_color_id(id) {
                    let cur = APP.with(|a| a.borrow().as_ref().map(|x| get_color(&x.cfg, id)));
                    if let Some(c) = cur {
                        if let Some(rgb) = choose_color(hwnd, (c.0, c.1, c.2)) {
                            APP.with(|a| {
                                if let Some(app) = a.borrow_mut().as_mut() {
                                    set_color(&mut app.cfg, id, rgb);
                                    app.cfg.save();
                                }
                            });
                        }
                    }
                }
                LRESULT(0)
            }
            WM_NOTIFY => {
                let nm = &*(lparam.0 as *const Nmhdr);
                if nm.code == TCN_SELCHANGE {
                    if let Ok(tab) = GetDlgItem(hwnd, ID_TAB) {
                        let sel = SendMessageW(tab, TCM_GETCURSEL, WPARAM(0), LPARAM(0)).0 as i32;
                        if sel >= 0 {
                            APP.with(|a| {
                                if let Some(app) = a.borrow().as_ref() {
                                    select_page(app, sel as usize);
                                }
                            });
                        }
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                let mut rc = RECT::default();
                if GetWindowRect(hwnd, &mut rc).is_ok() {
                    save_win_pos(rc.left, rc.top);
                }
                APP.with(|a| {
                    if let Some(app) = a.borrow_mut().as_mut() {
                        app.settings_hwnd = HWND(std::ptr::null_mut());
                        app.settings_pages = [Vec::new(), Vec::new()];
                    }
                });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

// ---------------- tray ----------------

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        ..Default::default()
    };
    unsafe {
        nid.hIcon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
    }
    let tip = "MouseSpot";
    for (i, c) in tip.encode_utf16().enumerate() {
        nid.szTip[i] = c;
    }
    nid
}

fn add_tray(hwnd: HWND) {
    let nid = tray_data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

fn remove_tray(hwnd: HWND) {
    let nid = tray_data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let menu = CreatePopupMenu().unwrap();

        let enabled = APP.with(|a| a.borrow().as_ref().map(|x| x.enabled).unwrap_or(true));
        let toggle_text = if enabled { w!("ハイライト: ON (クリックでOFF)") } else { w!("ハイライト: OFF (クリックでON)") };
        let _ = AppendMenuW(menu, MF_STRING, CMD_TOGGLE, toggle_text);
        let spot_on = APP.with(|a| a.borrow().as_ref().map(|x| x.cfg.spotlight_enabled).unwrap_or(false));
        let spot_text = if spot_on { w!("スポットライト: ON") } else { w!("スポットライト: OFF") };
        let _ = AppendMenuW(menu, MF_STRING, CMD_SPOT, spot_text);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS, w!("設定..."));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("終了"));

        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
    }
}
