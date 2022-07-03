#![allow(unused_parens)]
#![allow(non_snake_case)]

use core::mem::transmute;
use std::convert::TryInto;
use std::fs;
use std::os::raw::c_void;
use std::path::{Path, PathBuf};
use std::slice::from_raw_parts;
use std::str;
use std::{env, mem};
use windows::core::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::{Controls::*, Input::KeyboardAndMouse::EnableWindow, Shell::*, WindowsAndMessaging::*};
use windows::Win32::{Foundation::*, Graphics::Gdi::*, System::LibraryLoader::GetModuleHandleA};
// use windows::Win32::{System::Environment::GetCurrentDirectoryA};
use chrono::{prelude::Local, TimeZone};
use rusqlite::{Connection, Result};

include!("resource_defs.rs");

// Global Variables
// none?? well there are some, but they are defined later

fn main() -> Result<()> {
    println!("cargo:rustc-env=VERSION_STRING={}", env!("CARGO_PKG_VERSION"));
    /*
        let path_to_FileData_db=find_nx_studio_FileData_db();

        let nk_FileData_db = Connection::open(path_to_FileData_db.0).expect("Failed to load FileData.db database");
        if (path_to_FileData_db.1 == true)
        {
            println!("yersy");
        }
    */

    #[allow(non_upper_case_globals)]
    let mut test_studio: NxStudioDB = NxStudioDB { location: PathBuf::new(), success: false };

    if test_studio.existant() == true {
        println!("Yes");
    } else {
        println!("No");
    }

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

/// Dialog callback function for our main window
extern "system" fn main_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    #[allow(non_upper_case_globals)]
    static mut segoe_mdl2_assets: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) }; // Has to be global because we need to destroy our font resource eventually
    unsafe {
        let hinst = GetModuleHandleA(None);
        match nMsg as u32 {
            WM_INITDIALOG => {
                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon.unwrap().0));

                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(icon.unwrap().0));

                segoe_mdl2_assets.register_font(hwnd, "Segoe MDL2 Assets", 16, FW_NORMAL, false);
                segoe_mdl2_assets.set_text(IDC_ADD_PICTURE, "\u{EB9F}", "Add photo(s)\0");
                segoe_mdl2_assets.set_text(IDC_ADD_FOLDER, "\u{ED25}", "Add a folder full of photos\0");
                segoe_mdl2_assets.set_text(IDC_SAVE, "\u{E74E}", "Save changes to names\0");
                segoe_mdl2_assets.set_text(IDC_RENAME, "\u{E8AC}", "Manually rename selected photo\0");
                segoe_mdl2_assets.set_text(IDC_ERASE, "\u{ED60}", "Remove selected photo from the list\0");
                segoe_mdl2_assets.set_text(IDC_DELETE, "\u{ED62}", "Remove all photos from the list\0");
                segoe_mdl2_assets.set_text(IDC_INFO, "\u{E946}", "About\0");
                segoe_mdl2_assets.set_text(IDC_SETTINGS, "\u{F8B0}", "Settings\0");
                segoe_mdl2_assets.set_text(IDC_SYNC, "\u{EDAB}", "Resync names\0");

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
                    GetDlgItem(hwnd, IDC_FILE_LIST),
                    LVM_SETEXTENDEDLISTVIEWSTYLE,
                    WPARAM((LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER).try_into().unwrap()),
                    LPARAM((LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER).try_into().unwrap()),
                );

                let mut lvC = LVCOLUMNA {
                    mask: LVCF_FMT | LVCF_TEXT | LVCF_SUBITEM | LVCF_WIDTH,
                    fmt: LVCFMT_LEFT,
                    cx: convert_x_to_client_coords(IDC_FILE_LIST_R.width / 4),
                    pszText: transmute(utf8_to_utf16("Original File Name\0").as_ptr()),
                    cchTextMax: 0,
                    iSubItem: 0,
                    iImage: 0,
                    iOrder: 0,
                    cxMin: 50,
                    cxDefault: 55,
                    cxIdeal: 55,
                };

                SendMessageW(GetDlgItem(hwnd, IDC_FILE_LIST), LVM_INSERTCOLUMN, WPARAM(0), LPARAM(&lvC as *const _ as isize));

                lvC.iSubItem = 1;
                lvC.pszText = transmute(utf8_to_utf16("Changed File Name\0").as_ptr());
                SendMessageW(GetDlgItem(hwnd, IDC_FILE_LIST), LVM_INSERTCOLUMN, WPARAM(1), LPARAM(&lvC as *const _ as isize));

                lvC.pszText = transmute(utf8_to_utf16("File Created Time\0").as_ptr());
                SendMessageW(GetDlgItem(hwnd, IDC_FILE_LIST), LVM_INSERTCOLUMN, WPARAM(2), LPARAM(&lvC as *const _ as isize));

                lvC.pszText = transmute(utf8_to_utf16("Photo Taken Time\0").as_ptr());
                SendMessageW(GetDlgItem(hwnd, IDC_FILE_LIST), LVM_INSERTCOLUMN, WPARAM(3), LPARAM(&lvC as *const _ as isize));

                /*
                 * Check to see if we have a directory set up in LOCALAPPDATA
                 * If we don't have it yet, then we will try to create it
                 */

                let mut my_appdata = env::var("LOCALAPPDATA").expect("$LOCALAPPDATA is not set");
                my_appdata.push_str("\\exifrensc");
                let test_if_we_have_our_app_data_directory_set_up = PathBuf::from(&my_appdata);

                if test_if_we_have_our_app_data_directory_set_up.is_dir() {
                    println!("Yes!");
                } else {
                    fs::create_dir_all(test_if_we_have_our_app_data_directory_set_up).expect("failed to set up $LOCALAPPDATA");
                }

                0
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam); // I am sure there has to be a better way to do this, but the only way I could get the value out of a WPARAM type was to transmute it to a u64
                wParam = (wParam << 48 >> 48); // LOWORD isn't defined, at least as far as I could tell, so I had to improvise

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL {
                    segoe_mdl2_assets.destroy();
                    PostQuitMessage(0);
                } else if wParam as i32 == IDC_ADD_PICTURE {
                    LoadFile();
                } else if wParam as i32 == IDC_ADD_FOLDER {
                    LoadDirectory();
                } else if wParam as i32 == IDC_SAVE {
                    LoadDirectory();
                } else if wParam as i32 == IDC_SAVE {
                    LoadDirectory();
                } else if wParam as i32 == IDC_DELETE {
                    LoadDirectory();
                } else if wParam as i32 == IDC_ERASE {
                    LoadDirectory();
                } else if wParam as i32 == IDC_SYNC {
                    LoadDirectory();
                } else if wParam as i32 == IDC_SETTINGS {
                    CreateDialogParamA(hinst, PCSTR(IDD_SETTINGS as *mut u8), HWND(0), Some(settings_dlg_proc), LPARAM(0));
                } else if wParam as i32 == IDC_INFO {
                    CreateDialogParamA(hinst, PCSTR(IDD_ABOUT as *mut u8), HWND(0), Some(about_dlg_proc), LPARAM(0));
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
                    GetDlgItem(hwnd, IDC_FILE_LIST_R.id) as HWND,
                    HWND_TOP,
                    convert_x_to_client_coords(IDC_FILE_LIST_R.x),
                    convert_y_to_client_coords(IDC_FILE_LIST_R.y),
                    new_width - convert_x_to_client_coords(IDC_FILE_LIST_R.x + 8),
                    new_height - convert_y_to_client_coords(IDC_FILE_LIST_R.y + 8),
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                SetWindowPos(
                    GetDlgItem(hwnd, IDC_PATTERN_R.id) as HWND,
                    HWND_TOP,
                    convert_x_to_client_coords(IDC_PATTERN_R.x),
                    convert_y_to_client_coords(IDC_PATTERN_R.y),
                    new_width - convert_x_to_client_coords(IDC_PATTERN_R.x + 26),
                    convert_y_to_client_coords(IDC_PATTERN_R.height),
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                SetWindowPos(
                    GetDlgItem(hwnd, IDC_SYNC_R.id) as HWND,
                    HWND_TOP,
                    new_width - convert_x_to_client_coords(23),
                    convert_y_to_client_coords(IDC_PATTERN_R.y - 1),
                    convert_x_to_client_coords(IDC_SYNC_R.width),
                    convert_y_to_client_coords(IDC_SYNC_R.height),
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
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
                    println!("{}", file_name);
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

/// Dialog callback for our settings window
extern "system" fn settings_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    unsafe {
        match nMsg as u32 {
            WM_INITDIALOG => {
                let hinst = GetModuleHandleA(None);

                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon.unwrap().0));

                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(icon.unwrap().0));

                /*
                 * Set up our combo boxes
                 */
                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("Add\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("Skip\0").as_ptr())));
                SendMessageA(GetDlgItem(hwnd, IDC_ON_CONFLICT), CB_SETCURSEL, WPARAM(0), LPARAM(0));

                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT_ADD), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("_\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT_ADD), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("-\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT_ADD), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16(".\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT_ADD), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("~\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT_ADD), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("No delimeter\0").as_ptr())));
                SendMessageA(GetDlgItem(hwnd, IDC_ON_CONFLICT_ADD), CB_SETCURSEL, WPARAM(0), LPARAM(0));

                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT_NUM), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("12345\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT_NUM), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("1\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT_NUM), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("02\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_ON_CONFLICT_NUM), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("003\0").as_ptr())));
                SendMessageA(GetDlgItem(hwnd, IDC_ON_CONFLICT_NUM), CB_SETCURSEL, WPARAM(2), LPARAM(0));

                SendMessageW(GetDlgItem(hwnd, IDC_DATE_SHOOT_PRIMARY), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("the date shot in the EXIF data\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_DATE_SHOOT_PRIMARY), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("use \"File Created\" date\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_DATE_SHOOT_PRIMARY), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("use \"Last Modified\" date\0").as_ptr())));
                SendMessageA(GetDlgItem(hwnd, IDC_DATE_SHOOT_PRIMARY), CB_SETCURSEL, WPARAM(0), LPARAM(0));

                SendMessageW(GetDlgItem(hwnd, IDC_DATE_SHOOT_SECONDARY), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("use \"File Created\" date\0").as_ptr())));
                SendMessageW(GetDlgItem(hwnd, IDC_DATE_SHOOT_SECONDARY), CB_ADDSTRING, WPARAM(0), LPARAM(transmute(utf8_to_utf16("use \"Last Modified\" date\0").as_ptr())));
                SendMessageA(GetDlgItem(hwnd, IDC_DATE_SHOOT_SECONDARY), CB_SETCURSEL, WPARAM(0), LPARAM(0));

                /*
                 * Check to see if NX Studio is installed, and if it is, see if we can find the database file
                 */
                #[allow(non_upper_case_globals)]
                let mut NX_Studio: NxStudioDB = NxStudioDB { location: PathBuf::new(), success: false };

                if NX_Studio.existant() == false {
                    EnableWindow(GetDlgItem(hwnd, IDC_NX_STUDIO), false);
                    SendMessageA(GetDlgItem(hwnd, IDC_NX_STUDIO), BM_SETCHECK, WPARAM(BST_UNCHECKED.0.try_into().unwrap()), LPARAM(0));
                }

                0
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam); // I am sure there has to be a better way to do this, but the only way I could get the value out of a WPARAM type was to transmute it to a u64
                wParam = (wParam << 48 >> 48); // LOWORD isn't defined, at least as far as I could tell, so I had to improvise

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL || MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDOK {
                    EndDialog(hwnd, 0);
                }

                0
            }

            WM_DESTROY => {
                EndDialog(hwnd, 0);
                0
            }
            _ => 0,
        }
    }
}

/// Dialod callback for our about window
extern "system" fn about_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    // Have to be global because we need to destroy our font resources eventually
    #[allow(non_upper_case_globals)]
    static mut segoe_bold_9: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) };
    #[allow(non_upper_case_globals)]
    static mut segoe_bold_italic_13: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) };
    #[allow(non_upper_case_globals)]
    static mut segoe_italic_10: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) };
    unsafe {
        match nMsg as u32 {
            WM_INITDIALOG => {
                let hinst = GetModuleHandleA(None);

                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon.unwrap().0));

                let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
                SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(icon.unwrap().0));

                let annaversionary = chrono::Local.ymd(2022, 6, 17).and_hms(0, 0, 0);
                let majorversion = env!("CARGO_PKG_VERSION_MAJOR");
                let minorversion = env!("CARGO_PKG_VERSION_MINOR");
                let now = Local::now();
                let diff = now.signed_duration_since(annaversionary);
                let days = diff.num_days();
                let minutes = (diff.num_seconds() - (days * 86400)) / 60;
                let iso_8601 = now.format("%Y-%m-%d %H:%M").to_string();

                segoe_bold_9.register_font(hwnd, "Segoe UI", 9, FW_BOLD, false);
                segoe_bold_9.set_text(IDC_VER, "", "");
                segoe_bold_9.set_text(IDC_BUILT, "", "");
                segoe_bold_9.set_text(IDC_ST_AUTHOR, "", "");
                segoe_bold_9.set_text(IDC_ST_COPY, "", "");

                segoe_bold_italic_13.register_font(hwnd, "Segoe UI", 13, FW_BOLD, true);
                segoe_bold_italic_13.set_text(IDC_ABOUT_TITLE, "", "");

                segoe_italic_10.register_font(hwnd, "Segoe UI", 10, FW_NORMAL, true);
                segoe_italic_10.set_text(IDC_DESCRIPTION, "", "");

                SetDlgItemTextW(hwnd, IDC_VERSION, format!("{}.{}.{}.{}", majorversion, minorversion, days, minutes));
                SetDlgItemTextW(hwnd, IDC_BUILDDATE, iso_8601);

                0
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam); // I am sure there has to be a better way to do this, but the only way I could get the value out of a WPARAM type was to transmute it to a u64
                wParam = (wParam << 48 >> 48); // LOWORD isn't defined, at least as far as I could tell, so I had to improvise

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL || MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDOK {
                    segoe_bold_9.destroy();
                    segoe_bold_italic_13.destroy();
                    segoe_italic_10.destroy();
                    EndDialog(hwnd, 0);
                }

                0
            }

            WM_DESTROY => {
                segoe_bold_9.destroy();
                segoe_bold_italic_13.destroy();
                segoe_italic_10.destroy();
                EndDialog(hwnd, 0);
                0
            }
            _ => 0,
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
    fn register_font(&mut self, hwnd: HWND, face: &str, pitch: i32, weight: u32, italic: bool) {
        unsafe {
            let hdc = GetDC(hwnd);
            self.hfont = CreateFontA(
                (-1 * pitch * GetDeviceCaps(hdc, LOGPIXELSY)) / 72, // logical height of font
                0,                                                  // logical average character width
                0,                                                  // angle of escapement
                0,                                                  // base-line orientation angle
                weight.try_into().unwrap(),                         // font weight
                italic as u32,                                      // italic attribute flag
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
                let wide_text = utf8_to_utf16(tooltip_text);
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
                    hwnd: self.hwnd,                                     // Handle to the hwnd that contains the tool
                    uId: transmute(GetDlgItem(self.hwnd, id)),           // hwnd handle to the tool. or parent_hwnd
                    rect: RECT { left: 0, top: 0, right: 0, bottom: 0 }, // bounding rectangle coordinates of the tool, don't use, but seems to need to supply to stop it grumbling
                    hinst: hinst,                                        // Our hinstance
                    lpszText: transmute(wide_text.as_ptr()),             // Pointer to a utf16 buffer with the tooltip text
                    lParam: LPARAM(id.try_into().unwrap()),              // A 32-bit application-defined value that is associated with the tool
                    lpReserved: 0 as *mut c_void,                        // Reserved. Must be set to NULL
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
/// Convert a Rust utf8 string into a windows utf16 string
fn utf8_to_utf16(utf8_in: &str) -> Vec<u16> {
    utf8_in.encode_utf16().collect()
}

//fn LoadFile() -> Result<()> {
fn LoadFile() {
    println!("file open");
    unsafe {
        let file_dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).unwrap();

        // Change a few of the default options for the dialog
        file_dialog.SetTitle("Choose Photos to Rename");
        file_dialog.SetOkButtonLabel("Select Photos");
        //file_dialog.SetFileTypes();
        let mut options = file_dialog.GetOptions().unwrap();
        options.0 = options.0 | FOS_ALLOWMULTISELECT.0;
        file_dialog.SetOptions(options);

        let answer = file_dialog.Show(None); // Basically an error means no file was selected
                                             /*  if let Ok(__dummy) = answer {
                                                 let selected_file = file_dialog.GetResult().unwrap(); // IShellItem with the result. We know we have a result because we have got this far.
                                                 let file_name = selected_file.GetDisplayName(SIGDN_FILESYSPATH).unwrap(); // Pointer to a utf16 buffer with the file name
                                                 let tmp_slice = from_raw_parts(file_name.0, MAX_PATH as usize); // make the utf16 buffer look like a rust tmp_slice. This overruns, but that is okay.

                                                 // Figure out how big our file name is by walking the tmp_slice until we find the terminating null
                                                 // Really wish there was another way 😕
                                                 let mut item_name_len: usize = 0;
                                                 while tmp_slice[item_name_len] != 0 {
                                                     item_name_len += 1;
                                                 }


                                                 let tmp_file_name = from_raw_parts(file_name.0, item_name_len); // create another tmp_slice the size of the utf16 string
                                                 let mut file_name_s = String::from_utf16(tmp_file_name).unwrap(); // convert our utf16 buffer to a rust string
                                                 println!("{}", file_name_s);
                                                 CoTaskMemFree(transmute(file_name.0));
                                             } */

        // Multi selection version
        if let Ok(_dummy) = answer {
            let selected_files = file_dialog.GetResults().unwrap();
            let nSelected = selected_files.GetCount().unwrap();

            for i in 0..nSelected {
                let selected_file = selected_files.GetItemAt(i).unwrap();
                let file_name = selected_file.GetDisplayName(SIGDN_FILESYSPATH).unwrap();
                let tmp_slice = from_raw_parts(file_name.0, MAX_PATH as usize);
                let mut item_name_len: usize = 0;
                while tmp_slice[item_name_len] != 0 {
                    item_name_len += 1;
                }
                let tmp_file_name = from_raw_parts(file_name.0, item_name_len);
                let mut file_name_s = String::from_utf16(tmp_file_name).unwrap();
                println!("{}", file_name_s);
                CoTaskMemFree(transmute(file_name.0));
            }
        }

        //file_dialog.Release();
    }
    //    Ok(())
}

//fn LoadDirectory() -> Result<()> {
fn LoadDirectory() {
    println!("Directory open");
    unsafe {
        let file_dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).unwrap();

        file_dialog.SetTitle("Choose Directories of Photos to Add");
        file_dialog.SetOkButtonLabel("Select Directories");
        let mut options = file_dialog.GetOptions().unwrap();
        options.0 = options.0 | FOS_PICKFOLDERS.0 | FOS_ALLOWMULTISELECT.0;
        file_dialog.SetOptions(options);

        let answer = file_dialog.Show(None); // Basically an error means no file was selected
        if let Ok(_v) = answer {
            let selected_directories = file_dialog.GetResult().unwrap(); // IShellItem with the result. We know we have a result because we have got this far.
            let directory_name = selected_directories.GetDisplayName(SIGDN_FILESYSPATH).unwrap(); // Pointer to a utf16 buffer with the file name
            let tmp_slice = from_raw_parts(directory_name.0, MAX_PATH as usize); // make the utf16 buffer look like a rust tmp_slice. This overruns, but that is okay.

            // Figure out how big our file name is by walking the tmp_slice until we find the terminating null
            // Really wish there was another way 😕
            let mut item_name_len: usize = 0;
            while tmp_slice[item_name_len] != 0 {
                item_name_len += 1;
            }

            let tmp_directory_name = from_raw_parts(directory_name.0, item_name_len); // create another tmp_slice the size of the utf16 string
            let mut directory_name_s = String::from_utf16(tmp_directory_name).unwrap(); // convert our utf16 buffer to a rust string
            println!("{}", directory_name_s);
            CoTaskMemFree(transmute(directory_name.0));
        }

        //file_dialog.Release();
    }
    //    Ok(())
}

/// Function to find out of there are any user settings for NX Studio
///
/// Returns a PathBuff, which may be empty, so also check to see if it was successful or not
fn find_nx_studio_FileData_db() -> (PathBuf, bool) {
    let mut success = false;
    let mut localappdata = env::var("LOCALAPPDATA").expect("$LOCALAPPDATA is not set");
    localappdata.push_str("\\Nikon\\NX Studio\\DB\\FileData.db");
    let test_Path = PathBuf::from(&localappdata);

    /*
     * See if the file exists, if it does, change success to true
     */
    if test_Path.exists() {
        success = true;
    }
    (test_Path, success)
}

struct NxStudioDB {
    location: PathBuf,
    success: bool,
}

/// Functions pertaining to NX Studio's FileData.db
impl NxStudioDB {
    /// Check to see if FileData.db exists, if it does, set its location and return true, if it doesn't return false
    fn existant(&mut self) -> (bool) {
        if self.location.as_os_str() == "" {
            let mut success = false;
            let mut localappdata = env::var("LOCALAPPDATA").expect("$LOCALAPPDATA is not set");
            localappdata.push_str("\\Nikon\\NX Studio\\DB\\FileData.db");

            self.location = PathBuf::from(&localappdata);

            /*
             * See if the file exists, if it does, change success to true
             */
            if self.location.exists() {
                self.success = true;
            }
        }
        return self.success;
    }
}
