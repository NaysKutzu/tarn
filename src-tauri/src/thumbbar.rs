//! Windows taskbar thumbnail toolbar: Like / Previous / Play-Pause / Next,
//! the buttons under the preview when you hover the taskbar icon (Spotify-style).

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::models::PlayerState;

#[cfg(not(windows))]
pub fn start(_: Arc<crate::App>, _: AppHandle, _: Option<usize>) {}

#[cfg(not(windows))]
pub fn update(_: &PlayerState) {}

#[cfg(windows)]
pub fn start(app: Arc<crate::App>, handle: AppHandle, hwnd: Option<usize>) {
    let Some(hwnd) = hwnd else {
        eprintln!("thumbbar: no HWND");
        return;
    };
    let h = handle.clone();
    let _ = handle.run_on_main_thread(move || {
        if let Err(e) = imp::install(app, h, hwnd) {
            eprintln!("thumbbar: {e}");
        }
    });
}

#[cfg(windows)]
pub fn update(st: &PlayerState) {
    imp::update_playing(st.playing);
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

    const WM_COMMAND: u32 = 0x0111;
    const THBN_CLICKED: u32 = 0x1800;
    const THB_ICON: u32 = 0x2;
    const THB_TOOLTIP: u32 = 0x4;
    const THB_FLAGS: u32 = 0x8;
    const THBF_ENABLED: u32 = 0x0;
    const CLSCTX_INPROC_SERVER: u32 = 0x1;
    const MSGFLT_ALLOW: u32 = 1;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;

    const ID_LIKE: u32 = 1;
    const ID_PREV: u32 = 2;
    const ID_PLAY: u32 = 3;
    const ID_NEXT: u32 = 4;

    static PLAYING: AtomicBool = AtomicBool::new(false);
    static ADDED: AtomicBool = AtomicBool::new(false);
    static HWND: AtomicIsize = AtomicIsize::new(0);
    static CTX: AtomicIsize = AtomicIsize::new(0);

    #[repr(C)]
    struct ThumbButton {
        dw_mask: u32,
        i_id: u32,
        i_bitmap: u32,
        h_icon: isize,
        sz_tip: [u16; 260],
        dw_flags: u32,
    }

    struct Icons {
        like: isize,
        prev: isize,
        play: isize,
        pause: isize,
        next: isize,
    }

    struct Ctx {
        app: Arc<crate::App>,
        handle: AppHandle,
        hwnd: isize,
        icons: Icons,
        taskbar_msg: u32,
    }

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(pv: *mut c_void, dw: u32) -> i32;
        fn CoCreateInstance(
            clsid: *const Guid,
            outer: *mut c_void,
            ctx: u32,
            iid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterWindowMessageW(name: *const u16) -> u32;
        fn ChangeWindowMessageFilterEx(
            hwnd: *mut c_void,
            msg: u32,
            action: u32,
            change: *mut c_void,
        ) -> i32;
        fn CreateIconIndirect(info: *const IconInfo) -> isize;
        fn DestroyIcon(icon: isize) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateDIBSection(
            hdc: *mut c_void,
            bmi: *const BitmapInfo,
            usage: u32,
            bits: *mut *mut c_void,
            section: *mut c_void,
            offset: u32,
        ) -> isize;
        fn CreateBitmap(w: i32, h: i32, planes: u32, bpp: u32, bits: *const c_void) -> isize;
        fn DeleteObject(obj: isize) -> i32;
    }

    #[link(name = "comctl32")]
    extern "system" {
        fn SetWindowSubclass(
            hwnd: *mut c_void,
            pfn: SubclassFn,
            id: usize,
            data: usize,
        ) -> i32;
        fn DefSubclassProc(hwnd: *mut c_void, msg: u32, wp: usize, lp: isize) -> isize;
    }

    type SubclassFn = unsafe extern "system" fn(
        hwnd: *mut c_void,
        msg: u32,
        wp: usize,
        lp: isize,
        id: usize,
        data: usize,
    ) -> isize;

    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[repr(C)]
    struct IconInfo {
        f_icon: i32,
        x_hotspot: u32,
        y_hotspot: u32,
        hbm_mask: isize,
        hbm_color: isize,
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_pels: i32,
        y_pels: i32,
        clr_used: u32,
        clr_important: u32,
    }

    #[repr(C)]
    struct BitmapInfo {
        header: BitmapInfoHeader,
        colors: [u32; 3],
    }

    const CLSID_TASKBAR: Guid = Guid {
        data1: 0x56FDF344,
        data2: 0xFD6D,
        data3: 0x11D0,
        data4: [0x95, 0x8A, 0x00, 0x60, 0x97, 0xC9, 0xA0, 0x90],
    };
    const IID_TASKBAR3: Guid = Guid {
        data1: 0xEA1AFB91,
        data2: 0x9E28,
        data3: 0x4B86,
        data4: [0x90, 0xE9, 0x9E, 0x9F, 0x8A, 0x5E, 0xEF, 0xAF],
    };

    pub fn install(app: Arc<crate::App>, handle: AppHandle, hwnd: usize) -> Result<(), String> {
        let hwnd = hwnd as isize;
        HWND.store(hwnd, Ordering::Relaxed);
        unsafe {
            let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
        }

        let taskbar_msg = {
            let name: Vec<u16> = "TaskbarButtonCreated\0".encode_utf16().collect();
            unsafe { RegisterWindowMessageW(name.as_ptr()) }
        };
        if taskbar_msg != 0 {
            unsafe {
                ChangeWindowMessageFilterEx(
                    hwnd as *mut c_void,
                    taskbar_msg,
                    MSGFLT_ALLOW,
                    std::ptr::null_mut(),
                );
            }
        }

        let icons = Icons {
            like: make_icon(Glyph::Like).ok_or("like icon")?,
            prev: make_icon(Glyph::Prev).ok_or("prev icon")?,
            play: make_icon(Glyph::Play).ok_or("play icon")?,
            pause: make_icon(Glyph::Pause).ok_or("pause icon")?,
            next: make_icon(Glyph::Next).ok_or("next icon")?,
        };

        let ctx = Box::leak(Box::new(Ctx {
            app,
            handle,
            hwnd,
            icons,
            taskbar_msg,
        }));
        CTX.store(ctx as *mut Ctx as isize, Ordering::Release);

        let ok = unsafe {
            SetWindowSubclass(
                hwnd as *mut c_void,
                subclass_proc,
                0x74_41_52_4E, // "tARN"
                ctx as *mut Ctx as usize,
            )
        };
        if ok == 0 {
            return Err("SetWindowSubclass failed".into());
        }

        try_add(ctx);
        std::thread::Builder::new()
            .name("tarn-thumbbar".into())
            .spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(600));
                let p = CTX.load(Ordering::Acquire);
                if p == 0 {
                    return;
                }
                let ctx = unsafe { &*(p as *const Ctx) };
                unsafe {
                    let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
                }
                try_add(ctx);
            })
            .ok();

        eprintln!("thumbbar: subclass installed");
        Ok(())
    }

    pub fn update_playing(playing: bool) {
        let prev = PLAYING.swap(playing, Ordering::Relaxed);
        if prev == playing || !ADDED.load(Ordering::Relaxed) {
            return;
        }
        let p = CTX.load(Ordering::Acquire);
        if p == 0 {
            return;
        }
        let ctx = unsafe { &*(p as *const Ctx) };
        if let Err(e) = update_play_button(ctx, playing) {
            eprintln!("thumbbar: update: {e}");
        }
    }

    fn tip(s: &str) -> [u16; 260] {
        let mut out = [0u16; 260];
        for (i, c) in s.encode_utf16().take(259).enumerate() {
            out[i] = c;
        }
        out
    }

    fn buttons(icons: &Icons, playing: bool) -> [ThumbButton; 4] {
        let play_icon = if playing { icons.pause } else { icons.play };
        let play_tip = if playing { "Pause" } else { "Play" };
        let mask = THB_ICON | THB_TOOLTIP | THB_FLAGS;
        [
            ThumbButton {
                dw_mask: mask,
                i_id: ID_LIKE,
                i_bitmap: 0,
                h_icon: icons.like,
                sz_tip: tip("Like"),
                dw_flags: THBF_ENABLED,
            },
            ThumbButton {
                dw_mask: mask,
                i_id: ID_PREV,
                i_bitmap: 0,
                h_icon: icons.prev,
                sz_tip: tip("Previous"),
                dw_flags: THBF_ENABLED,
            },
            ThumbButton {
                dw_mask: mask,
                i_id: ID_PLAY,
                i_bitmap: 0,
                h_icon: play_icon,
                sz_tip: tip(play_tip),
                dw_flags: THBF_ENABLED,
            },
            ThumbButton {
                dw_mask: mask,
                i_id: ID_NEXT,
                i_bitmap: 0,
                h_icon: icons.next,
                sz_tip: tip("Next"),
                dw_flags: THBF_ENABLED,
            },
        ]
    }

    fn try_add(ctx: &Ctx) {
        if ADDED.load(Ordering::Relaxed) {
            return;
        }
        match add_buttons(ctx.hwnd, &ctx.icons, PLAYING.load(Ordering::Relaxed)) {
            Ok(()) => {
                ADDED.store(true, Ordering::Relaxed);
                eprintln!("thumbbar: buttons attached");
            }
            Err(e) => eprintln!("thumbbar: add deferred ({e})"),
        }
    }

    fn add_buttons(hwnd: isize, icons: &Icons, playing: bool) -> Result<(), String> {
        let mut btns = buttons(icons, playing);
        with_taskbar(|tb| unsafe {
            let hr = thumb_bar_add_buttons(tb, hwnd as *mut c_void, 4, btns.as_mut_ptr());
            if hr < 0 {
                Err(format!("ThumbBarAddButtons 0x{hr:08X}"))
            } else {
                Ok(())
            }
        })
    }

    fn update_play_button(ctx: &Ctx, playing: bool) -> Result<(), String> {
        let mut btn = ThumbButton {
            dw_mask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
            i_id: ID_PLAY,
            i_bitmap: 0,
            h_icon: if playing {
                ctx.icons.pause
            } else {
                ctx.icons.play
            },
            sz_tip: tip(if playing { "Pause" } else { "Play" }),
            dw_flags: THBF_ENABLED,
        };
        with_taskbar(|tb| unsafe {
            let hr = thumb_bar_update_buttons(tb, ctx.hwnd as *mut c_void, 1, &mut btn);
            if hr < 0 {
                Err(format!("ThumbBarUpdateButtons 0x{hr:08X}"))
            } else {
                Ok(())
            }
        })
    }

    fn with_taskbar<R>(f: impl FnOnce(*mut c_void) -> Result<R, String>) -> Result<R, String> {
        unsafe {
            let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
            let mut punk: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_TASKBAR,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_TASKBAR3,
                &mut punk,
            );
            if hr < 0 || punk.is_null() {
                return Err(format!("CoCreateInstance 0x{hr:08X}"));
            }
            let hr_init = vcall0(punk, 3);
            if hr_init < 0 {
                release(punk);
                return Err(format!("HrInit 0x{hr_init:08X}"));
            }
            let out = f(punk);
            release(punk);
            out
        }
    }

    unsafe fn vtable(this: *mut c_void) -> *mut *mut c_void {
        *(this as *mut *mut *mut c_void)
    }

    unsafe fn vcall0(this: *mut c_void, idx: usize) -> i32 {
        let f: unsafe extern "system" fn(*mut c_void) -> i32 =
            std::mem::transmute(*vtable(this).add(idx));
        f(this)
    }

    unsafe fn release(this: *mut c_void) {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*vtable(this).add(2));
        f(this);
    }

    unsafe fn thumb_bar_add_buttons(
        this: *mut c_void,
        hwnd: *mut c_void,
        count: u32,
        buttons: *mut ThumbButton,
    ) -> i32 {
        let f: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut ThumbButton) -> i32 =
            std::mem::transmute(*vtable(this).add(15));
        f(this, hwnd, count, buttons)
    }

    unsafe fn thumb_bar_update_buttons(
        this: *mut c_void,
        hwnd: *mut c_void,
        count: u32,
        buttons: *mut ThumbButton,
    ) -> i32 {
        let f: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut ThumbButton) -> i32 =
            std::mem::transmute(*vtable(this).add(16));
        f(this, hwnd, count, buttons)
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: *mut c_void,
        msg: u32,
        wp: usize,
        lp: isize,
        _id: usize,
        data: usize,
    ) -> isize {
        let ctx = &*(data as *const Ctx);
        if msg == ctx.taskbar_msg && ctx.taskbar_msg != 0 {
            try_add(ctx);
        } else if msg == WM_COMMAND {
            let hi = ((wp >> 16) & 0xFFFF) as u32;
            let id = (wp & 0xFFFF) as u32;
            if hi == THBN_CLICKED {
                on_click(ctx, id);
                return 0;
            }
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }

    fn on_click(ctx: &Ctx, id: u32) {
        use souvlaki::MediaControlEvent as E;
        match id {
            ID_LIKE => {
                let _ = ctx.handle.emit("tray", "like");
            }
            ID_PREV => crate::mpris::dispatch(&ctx.app, E::Previous),
            ID_PLAY => crate::mpris::dispatch(&ctx.app, E::Toggle),
            ID_NEXT => crate::mpris::dispatch(&ctx.app, E::Next),
            _ => {}
        }
    }

    #[derive(Clone, Copy)]
    enum Glyph {
        Like,
        Prev,
        Play,
        Pause,
        Next,
    }

    fn make_icon(glyph: Glyph) -> Option<isize> {
        const N: i32 = 16;
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bmi = BitmapInfo {
            header: BitmapInfoHeader {
                size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                width: N,
                height: -N,
                planes: 1,
                bit_count: 32,
                compression: 0,
                size_image: 0,
                x_pels: 0,
                y_pels: 0,
                clr_used: 0,
                clr_important: 0,
            },
            colors: [0, 0, 0],
        };
        let color = unsafe {
            CreateDIBSection(
                std::ptr::null_mut(),
                &bmi,
                0,
                &mut bits,
                std::ptr::null_mut(),
                0,
            )
        };
        if color == 0 || bits.is_null() {
            return None;
        }
        let px = unsafe { std::slice::from_raw_parts_mut(bits as *mut u32, (N * N) as usize) };
        px.fill(0);
        draw(px, N as usize, glyph);

        let mask_bits = vec![0u8; ((N * N) / 8) as usize];
        let mask =
            unsafe { CreateBitmap(N, N, 1, 1, mask_bits.as_ptr() as *const c_void) };
        if mask == 0 {
            unsafe {
                DeleteObject(color);
            }
            return None;
        }
        let info = IconInfo {
            f_icon: 1,
            x_hotspot: 0,
            y_hotspot: 0,
            hbm_mask: mask,
            hbm_color: color,
        };
        let icon = unsafe { CreateIconIndirect(&info) };
        unsafe {
            DeleteObject(color);
            DeleteObject(mask);
        }
        if icon == 0 {
            None
        } else {
            Some(icon)
        }
    }

    fn put(px: &mut [u32], n: usize, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && (x as usize) < n && (y as usize) < n {
            px[y as usize * n + x as usize] = c;
        }
    }

    fn draw(px: &mut [u32], n: usize, glyph: Glyph) {
        // Opaque white in BGRA (AARRGGBB as u32 LE).
        let w = 0xFFFFFFFFu32;
        match glyph {
            Glyph::Play => {
                for y in 3..13 {
                    let t = (y - 3) as f32 / 9.0;
                    let left = 5;
                    let right = 5 + ((1.0 - (t - 0.5).abs() * 2.0) * 6.0) as i32;
                    for x in left..=right {
                        put(px, n, x, y, w);
                    }
                }
            }
            Glyph::Pause => {
                for y in 3..13 {
                    for x in 4..7 {
                        put(px, n, x, y, w);
                    }
                    for x in 9..12 {
                        put(px, n, x, y, w);
                    }
                }
            }
            Glyph::Prev => {
                for y in 3..13 {
                    put(px, n, 3, y, w);
                    put(px, n, 4, y, w);
                }
                for y in 3..13 {
                    let t = (y - 3) as f32 / 9.0;
                    let right = 12;
                    let left = 12 - ((1.0 - (t - 0.5).abs() * 2.0) * 6.0) as i32;
                    for x in left..=right {
                        put(px, n, x, y, w);
                    }
                }
            }
            Glyph::Next => {
                for y in 3..13 {
                    put(px, n, 11, y, w);
                    put(px, n, 12, y, w);
                }
                for y in 3..13 {
                    let t = (y - 3) as f32 / 9.0;
                    let left = 3;
                    let right = 3 + ((1.0 - (t - 0.5).abs() * 2.0) * 6.0) as i32;
                    for x in left..=right {
                        put(px, n, x, y, w);
                    }
                }
            }
            Glyph::Like => {
                for y in 0..n as i32 {
                    for x in 0..n as i32 {
                        let fx = x as f32 - 7.5;
                        let fy = y as f32 - 7.0;
                        let a = ((fx + 2.2).powi(2) + (fy + 1.2).powi(2)).sqrt() < 2.6;
                        let b = ((fx - 2.2).powi(2) + (fy + 1.2).powi(2)).sqrt() < 2.6;
                        let c = fy > -0.5 && (fx.abs() + (fy - 1.5) * 0.85) < 5.2;
                        if a || b || c {
                            put(px, n, x, y, w);
                        }
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    fn destroy_icons(icons: &Icons) {
        unsafe {
            DestroyIcon(icons.like);
            DestroyIcon(icons.prev);
            DestroyIcon(icons.play);
            DestroyIcon(icons.pause);
            DestroyIcon(icons.next);
        }
    }
}
