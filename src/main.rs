#![allow(unused_parens)]
#![allow(non_snake_case)]

use core::mem::transmute;
use std::convert::TryInto;
use std::mem;
use std::os::raw::c_void;
use windows::core::*;
use windows::Win32::UI::{Controls::*, Shell::*, WindowsAndMessaging::*};
use windows::Win32::{Foundation::*, Graphics::Gdi::*, System::LibraryLoader::GetModuleHandleA};
// use windows::Win32::{System::Environment::GetCurrentDirectoryA};

include!("resource_defs.rs");

// Global Variables

//const VERSION_STRING: &'static str = env!("VERSION_STRING");

fn main() -> Result<()> {
    unsafe {
        InitCommonControls();
        let hinst = GetModuleHandleA(None);
        let main_hwnd = CreateDialogParamA(hinst, PCSTR(IDD_MAIN as *mut u8), HWND(0), Some(main_dlg_proc), LPARAM(0));
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

extern "system" fn main_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    static mut segoe_mdl2_assets: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) }; // Has to be global because we need to destroy our font resource eventually
    unsafe {
        match nMsg as u32 {
            WM_INITDIALOG => {
                let hinst = GetModuleHandleA(None);

                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon.unwrap().0));

                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(icon.unwrap().0));

                segoe_mdl2_assets.register_font(hwnd, "Segoe MDL2 Assets", 16);
                segoe_mdl2_assets.set_text(IDC_ADD_PICTURE.id, "\u{EB9F}", "Add photo(s)\0");
                segoe_mdl2_assets.set_text(IDC_RENAME.id, "\u{E8AC}", "Manually rename selected photo\0");
                segoe_mdl2_assets.set_text(IDC_ERASE.id, "\u{ED60}", "Remove photo from the list\0");
                segoe_mdl2_assets.set_text(IDC_DELETE.id, "\u{E74D}", "Remove all photos\0");

                //DragAcceptFiles(GetDlgItem(hwnd, IDC_FILE_LIST) as HWND, true);

                /*
                                    let mut file_name_buffer = [0; MAX_PATH as usize];
                                    GetCurrentDirectoryA(file_name_buffer.as_mut_slice());
                                    DlgDirListA(hwnd,
                                        transmute(&file_name_buffer[0]),
                                        40004,
                                        0,
                                        DDL_DRIVES|DDL_DIRECTORY
                                      );
                */

                /*
                 * Setup up our listview
                 */

                SendMessageW(
                    GetDlgItem(hwnd, IDC_FILE_LIST.id),
                    LVM_SETEXTENDEDLISTVIEWSTYLE,
                    WPARAM(
                        (LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER)
                            .try_into()
                            .unwrap(),
                    ),
                    LPARAM(
                        (LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER)
                            .try_into()
                            .unwrap(),
                    ),
                );

                let wide_text: Vec<u16> = "Original File Name\0".encode_utf16().collect();
                let mut lvC = LVCOLUMNA {
                    mask: LVCF_FMT | LVCF_TEXT | LVCF_SUBITEM | LVCF_WIDTH,
                    fmt: LVCFMT_LEFT,
                    cx: convert_x_to_client_coords(IDC_FILE_LIST.width / 4),
                    pszText: transmute(wide_text.as_ptr()),
                    cchTextMax: 0,
                    iSubItem: 0,
                    iImage: 0,
                    iOrder: 0,
                    cxMin: 50,
                    cxDefault: 55,
                    cxIdeal: 55,
                };

                SendMessageW(GetDlgItem(hwnd, IDC_FILE_LIST.id), LVM_INSERTCOLUMN, WPARAM(0), LPARAM(&lvC as *const _ as isize));

                lvC.iSubItem = 1;
                let wide_text: Vec<u16> = "Changed File Name\0".encode_utf16().collect();
                lvC.pszText = transmute(wide_text.as_ptr());
                SendMessageW(GetDlgItem(hwnd, IDC_FILE_LIST.id), LVM_INSERTCOLUMN, WPARAM(1), LPARAM(&lvC as *const _ as isize));

                let wide_text: Vec<u16> = "File Created Time\0".encode_utf16().collect();
                lvC.pszText = transmute(wide_text.as_ptr());
                SendMessageW(GetDlgItem(hwnd, IDC_FILE_LIST.id), LVM_INSERTCOLUMN, WPARAM(2), LPARAM(&lvC as *const _ as isize));

                let wide_text: Vec<u16> = "Photo Taken Time\0".encode_utf16().collect();
                lvC.pszText = transmute(wide_text.as_ptr());
                SendMessageW(GetDlgItem(hwnd, IDC_FILE_LIST.id), LVM_INSERTCOLUMN, WPARAM(3), LPARAM(&lvC as *const _ as isize));

                0
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam); // I am sure there has to be a better way to do this, but the only way I could get the value out of a WPARAM type was to transmute it to a u64
                wParam = (wParam << 48 >> 48); // LOWORD isn't defined, at least as far as I could tell, so I had to improvise

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL {
                    println!("{}", wParam);
                    segoe_mdl2_assets.destroy();
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

                // if MapDialogRect(hwnd,&mut *borrowed_rect) == true
                //    {
                //     SetWindowPos( GetDlgItem(hwnd, IDC_FILE_LIST) as HWND, HWND_TOP,
                //                   borrowed_rect.left,borrowed_rect.top,
                //                   borrowed_rect.right-borrowed_rect.left,borrowed_rect.bottom-borrowed_rect.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                //     }

                // Because that didn't work as advertised, perhaps because I am using Segoe UI as the font instead of the default font,
                // which is MS Shell Dialog and dates back to XP (or earlier?), I calculate the resizing manually based on Segoe UI.
                // I am not sure what effects this might have on other monitors with different resolutions of DPI settings.

                SetWindowPos(
                    GetDlgItem(hwnd, IDC_FILE_LIST.id) as HWND,
                    HWND_TOP,
                    convert_x_to_client_coords(IDC_FILE_LIST.x),
                    convert_y_to_client_coords(IDC_FILE_LIST.y),
                    new_width - convert_x_to_client_coords(IDC_FILE_LIST.x + 8),
                    new_height - convert_y_to_client_coords(IDC_FILE_LIST.y + 8),
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                SetWindowPos(
                    GetDlgItem(hwnd, IDC_PATTERN.id) as HWND,
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
                let nFiles: u32 = DragQueryFileA(hDrop, 0xFFFFFFFF, file_name_buffer.as_mut_slice()); // Wish I could send a NULL as the last param since I don't really need to pass a buffer for this call

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
            //_ => DefDlgProcA(hwnd, message, wParam, lParam).0,
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
    (height * 1925 / 1000) // had been 1850, but 1925 produces slightly better results
}

struct WindowsControlText {
    hwnd: HWND,
    hfont: HFONT,
}

impl WindowsControlText {
    /**
     * Register a font and size
     **/
    fn register_font(&mut self, hwnd: HWND, face: &str, pitch: i32) {
        unsafe {
            let hdc = GetDC(hwnd);
            self.hfont = CreateFontA(
                (-1 * pitch * GetDeviceCaps(hdc, LOGPIXELSY)) / 72, // logical height of font
                0,                                                  // logical average character width
                0,                                                  // angle of escapement
                0,                                                  // base-line orientation angle
                FW_NORMAL as i32,                                   // font weight
                0,                                                  // italic attribute flag
                0,                                                  // underline attribute flag
                0,                                                  // strikeout attribute flag
                ANSI_CHARSET,                                       // character set identifier
                OUT_DEFAULT_PRECIS,                                 // output precision
                CLIP_DEFAULT_PRECIS,                                // clipping precision
                PROOF_QUALITY,                                      // output quality
                FF_DECORATIVE,                                      // pitch and family
                face,                                               // pointer to typeface name string
            );
            self.hwnd = hwnd;
            ReleaseDC(hwnd, hdc);
        }
    }

    /**
     * Set the caption and tool tip text of a windows control.
     **/
    fn set_text(&self, id: i32, caption: &str, tooltip_text: &str) {
        unsafe {
            SendDlgItemMessageA(self.hwnd, id, WM_SETFONT, WPARAM(self.hfont.0 as usize), LPARAM(0));

            if caption != "" {
                SetDlgItemTextW(self.hwnd, id, caption);
            }

            if tooltip_text != "" {
                let wide_text: Vec<u16> = tooltip_text.encode_utf16().collect();
                let hinst = GetModuleHandleA(None);

                let tt_hwnd = CreateWindowExA(
                    Default::default(),
                    TOOLTIPS_CLASS,
                    None,
                    WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP), // | WINDOW_STYLE(TTS_BALLOON), // I don't really like the balloon style, but this is how we'd define it
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    self.hwnd,
                    None,
                    hinst,
                    std::ptr::null(),
                );

                let toolInfo = TTTOOLINFOA {
                    cbSize: mem::size_of::<TTTOOLINFOA>() as u32,
                    uFlags: TTF_IDISHWND | TTF_SUBCLASS,
                    hwnd: self.hwnd,                           // Handle to the hwnd that contains the tool
                    uId: transmute(GetDlgItem(self.hwnd, id)), // hwnd handle to the tool. or parent_hwnd
                    rect: RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    },                                         // bounding rectangle coordinates of the tool, don't use, but seems to need to supply to stop it grumbling
                    hinst: hinst,                              // Our hinstance
                    lpszText: transmute(wide_text.as_ptr()),   // Pointer to a utf16 buffer with the tooltip text
                    lParam: LPARAM(id.try_into().unwrap()),    // A 32-bit application-defined value that is associated with the tool
                    lpReserved: 0 as *mut c_void,              // Reserved. Must be set to NULL
                };

                SendMessageA(tt_hwnd, TTM_ADDTOOL, WPARAM(0), LPARAM(&toolInfo as *const _ as isize));
                SendMessageA(tt_hwnd, TTM_SETMAXTIPWIDTH, WPARAM(0), LPARAM(200));
            }
        }
    }

    /**
     *  Delete the font resource when we are done with it
     **/
    fn destroy(&self) {
        unsafe {
            DeleteObject(self.hfont);
        }
    }
}
