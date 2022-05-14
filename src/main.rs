#![allow(unused_parens)]
#![allow(non_snake_case)]

use std::convert::TryInto;
use std::mem;
use std::os::raw::c_void;
use ::core::mem::transmute;
use windows::core::*;
use windows::Win32::UI::{Controls::*, Shell::*, WindowsAndMessaging::*};
use windows::Win32::{Foundation::*, Graphics::Gdi::*, System::LibraryLoader::GetModuleHandleA};

include!("resource_defs.rs");

//const VERSION_STRING: &'static str = env!("VERSION_STRING");

fn main() -> Result<()> {
    unsafe {
        InitCommonControls();
        let hinst = GetModuleHandleA(None);
        let main_hwnd = CreateDialogParamA(
            hinst,
            PCSTR(IDD_MAIN as *mut u8),
            HWND(0),
            Some(main_dlg_proc),
            LPARAM(0),
        );
        let mut message = MSG::default();

        while GetMessageA(&mut message, HWND(0), 0, 0).into() {
            if (IsDialogMessageA(main_hwnd, &message) == false) {
                TranslateMessage(&message);
                DispatchMessageA(&message);
            }
        }

        Ok(())
    }
}

extern "system" fn main_dlg_proc(
    window: HWND,
    message: u32,
    wParam: WPARAM,
    lParam: LPARAM,
) -> isize {
    unsafe {
        match message as u32 {
            WM_INITDIALOG => {
                let hinst = GetModuleHandleA(None);
                let hdc = GetDC(window);
                let segoe_ui_symbol = CreateFontA(
                    (-16 * GetDeviceCaps(hdc, LOGPIXELSY)) / 72, // logical height of font
                    0,                                           // logical average character width
                    0,                                           // angle of escapement
                    0,                                           // base-line orientation angle
                    FW_NORMAL as i32,                            // font weight
                    0,                                           // italic attribute flag
                    0,                                           // underline attribute flag
                    0,                                           // strikeout attribute flag
                    ANSI_CHARSET,                                // character set identifier
                    OUT_DEFAULT_PRECIS,                          // output precision
                    CLIP_DEFAULT_PRECIS,                         // clipping precision
                    PROOF_QUALITY,                               // output quality
                    FF_DECORATIVE,                               // pitch and family
                    "Segoe UI Symbol",                           // pointer to typeface name string
                );

                SendDlgItemMessageA(
                    window,
                    IDC_ADD_PICTURE.id,
                    WM_SETFONT,
                    WPARAM(segoe_ui_symbol.0 as usize),
                    LPARAM(0),
                );
                
                SetDlgItemTextW(window, IDC_ADD_PICTURE.id, ""); // Picture
                //DeleteObject(segoe_ui_symbol);

                AddToolTip(window, IDC_ADD_PICTURE.id, "Add picture(s)\0");


                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(
                    window,
                    WM_SETICON,
                    WPARAM(ICON_BIG as usize),
                    LPARAM(icon.unwrap().0),
                );

                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(
                    window,
                    WM_SETICON,
                    WPARAM(ICON_SMALL as usize),
                    LPARAM(icon.unwrap().0),
                );

                //DragAcceptFiles(GetDlgItem(window, IDC_FILE_LIST) as HWND, true);

                ReleaseDC(window, hdc);

                0
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam); // I am sure there has to be a better way to do this, but the only way I could get the value out of a WPARAM type was to transmute it to a u64
                wParam = (wParam << 48 >> 48); // LOWORD isn't defined, at least as far as I could tell, so I had to improvise

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL {
                    println!("{}", wParam);
                    PostQuitMessage(0);
                }
                0
            }

            WM_SIZE => {
                let mut new_width: u64 = transmute(lParam);
                new_width = (new_width << 48 >> 48); // LOWORD
                let new_width: i32 = new_width.try_into().unwrap();
                let mut new_height: u64 = transmute(lParam);
                new_height = (new_height << 32 >> 48); // HIWORD
                let new_height: i32 = new_height.try_into().unwrap();

                // In theory, this should work, but it doesn't 😥, so I am not at sure at all what I am doing wrong, but the recomputed
                // values for the top of the rectangle are correct, but the right and bottom are out by quite a bit.😒
                //
                // let mut original_rect = RECT{left:8, top:106, right:new_width-16,bottom:new_height-16};
                // let borrowed_rect=&mut original_rect;

                // if MapDialogRect(window,&mut *borrowed_rect) == true
                //    {
                //     SetWindowPos( GetDlgItem(window, IDC_FILE_LIST) as HWND, HWND_TOP,
                //                   borrowed_rect.left,borrowed_rect.top,
                //                   borrowed_rect.right-borrowed_rect.left,borrowed_rect.bottom-borrowed_rect.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                //     }

                // Because that didn't work as advertised, perhaps because I am using Segoe UI as the font instead of the default font,
                // which is MS Shell Dialog and dates back to XP (or earlier?), I calculate the resizing manually based on Segoe UI.
                // I am not sure what effects this might have on other monitors with different resolutions of DPI settings.

                SetWindowPos(
                    GetDlgItem(window, IDC_FILE_LIST.id) as HWND,
                    HWND_TOP,
                    convert_x_to_client_coords(IDC_FILE_LIST.x),
                    convert_y_to_client_coords(IDC_FILE_LIST.y),
                    new_width - convert_x_to_client_coords(IDC_FILE_LIST.x + 8),
                    new_height - convert_y_to_client_coords(IDC_FILE_LIST.y + 8),
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                SetWindowPos(
                    GetDlgItem(window, IDC_PATTERN.id) as HWND,
                    HWND_TOP,
                    convert_x_to_client_coords(IDC_PATTERN.x),
                    convert_y_to_client_coords(IDC_PATTERN.y),
                    new_width - convert_x_to_client_coords(IDC_PATTERN.x + 8),
                    convert_y_to_client_coords(IDC_PATTERN.height),
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                0
            }

            WM_DROPFILES => {
                let mut file_name_buffer = [0; MAX_PATH as usize];
                let hDrop: HDROP = HDROP(transmute(wParam));
                let nFiles: u32 =
                    DragQueryFileA(hDrop, 0xFFFFFFFF, file_name_buffer.as_mut_slice()); // Wish I could send a NULL as the last param since I don't really need to pass a buffer for this call

                for i in 0..nFiles
                // Walk through the dropped "files" one by one, but they may not all be files, some may be directories 😛
                {
                    DragQueryFileA(hDrop, i, file_name_buffer.as_mut_slice());
                    let mut file_name = String::from_utf8_unchecked((&file_name_buffer).to_vec());
                    file_name.truncate(file_name.find('\0').unwrap());
                }

                DragFinish(hDrop);
                0
            }

            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => 0,
            //_ => DefDlgProcA(window, message, wParam, lParam).0,
        }
    }
}

/// Converts width to client width based on the Seogoe UI font's average size
/// 
/// The values were hand computed and may not work for all monitors, but it works on all the ones I have to check.
fn convert_x_to_client_coords(width: i32) -> (i32) {
    (width * 1750 / 1000)
}

/// Converts width to client height based on the Seogoe UI font's average size
/// 
/// The values were hand computed and may not work for all monitors, but it works on all the ones I have to check.
fn convert_y_to_client_coords(height: i32) -> (i32) {
    (height * 1875 / 1000)
}

// Description:
//   Creates a tooltip for an item in a dialog box.
// Parameters:
//   idTool - identifier of an dialog box item.
//   nDlg - window handle of the dialog box.
//   text - string to use as the tooltip text.

fn AddToolTip(parent_hwnd: HWND, dlg_ID: i32, text: &str) -> (HWND) {    
    unsafe {
        let textu16: Vec<u16> = text.encode_utf16().collect();
        let hinst = GetModuleHandleA(None);

        let tt_hwnd = CreateWindowExA(
            Default::default(),
            TOOLTIPS_CLASS,
            None,
            WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP) , //| WINDOW_STYLE(TTS_BALLOON),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            parent_hwnd,
            None,
            hinst,
            std::ptr::null(),
        );

        let toolInfo = TTTOOLINFOA {
            cbSize: mem::size_of::<TTTOOLINFOA>() as u32,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            hwnd: parent_hwnd,                                   // Handle to the window that contains the tool
            uId: transmute(GetDlgItem(parent_hwnd, dlg_ID)),     // window handle to the tool. or parent_hwnd
            rect: RECT { left: 0, top: 0, right: 0, bottom: 0 }, // bounding rectangle coordinates of the tool, don't use, but seems to need to supply to stop it grumbling
            hinst: hinst,                                        // Oue hinstance
            lpszText: transmute(textu16.as_ptr()),                  // Pointer to a utf16 buffer with the tooltip teext
            lParam: LPARAM(dlg_ID.try_into().unwrap()),          // A 32-bit application-defined value that is associated with the tool
            lpReserved: 0 as *mut c_void,                        // Reserved. Must be set to NULL
        };

        SendMessageA(
            tt_hwnd,
            TTM_ADDTOOL,
            WPARAM(0),
            LPARAM(&toolInfo as *const _ as isize),
        );
        SendMessageA(tt_hwnd, TTM_SETMAXTIPWIDTH, WPARAM(0), LPARAM(200));

        tt_hwnd //
    }
}
