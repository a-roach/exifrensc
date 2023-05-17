#![allow(unused_parens)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use core::mem::transmute;
use std::collections::HashMap;
use std::convert::TryInto;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::File;
use std::io::Write;
use std::os::raw::c_void;
use std::path::{Path, PathBuf};
use std::thread;
use std::{env, mem, slice, slice::from_raw_parts, str};
use windows::core::*;
use windows::Win32::UI::{
    Controls::{LIST_VIEW_ITEM_STATE_FLAGS, LVITEMA_GROUP_ID, *},
    Input::KeyboardAndMouse::EnableWindow,
    Shell::{Common::COMDLG_FILTERSPEC, *},
    WindowsAndMessaging::*,
};
use windows::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{Com::*, LibraryLoader::*},
};
// use windows::Win32::UI::Shell::SHCreateItemInKnownFolder;
// use windows::Win32::{System::Environment::GetCurrentDirectoryA};
use chrono::{prelude::Local, DateTime, TimeZone};
use rand::prelude::*;
use rusqlite::{Connection, Result};
use tiny_http::{Response, Server};
use urlencoding::decode;

include!("resource_defs.rs");

// Custom Macros

macro_rules! Warning {
    ($a:expr) => {
        unsafe {
            MessageBoxA(None, s!($a), s!("Warning!"), MB_OK | MB_ICONINFORMATION);
        }
    };
}

macro_rules! sWarning {
    ($a:expr) => {
        unsafe {
            MessageBoxA(None, s!($a), s!("Warning!"), MB_OK | MB_ICONINFORMATION);
        }
    };
}

macro_rules! Fail {
    ($a:expr) => {
        unsafe {
            MessageBoxA(None, s!($a), s!("Error!"), MB_OK | MB_ICONERROR);
        }
    };
}

macro_rules! FailU {
    ($a:expr) => {
        MessageBoxA(None, s!($a), s!("Error!"), MB_OK | MB_ICONERROR);
    };
}

// Global Variables
static mut path_to_settings_sqlite: String = String::new();
static mut main_hwnd: HWND = windows::Win32::Foundation::HWND(0);
static mut BONAFIDE: String = String::new(); // Used for verifying that the internal web server got a bonafide response from within the program
static mut EXITERMINATE: bool = false; // used to signal when our web server has been potentially compromised
pub const HOST: &str = "127.0.0.1:18792";
pub const HOST_URL: &str = "http://127.0.0.1:18792";

// Some definitions seemingly missing, as of coding, from the windows crate
pub const NM_CLICK: u32 = 4294967195;
pub const NM_DBLCLK: u32 = 4294967294;
pub const NM_RCLICK: u32 = 4294967291;
pub const NM_RDBLCLK: u32 = 4294967290;

pub const ID_CANCEL: i32 = 2; // This define just makes life easier, because IDCANCEL is defined in a really odd way

/// Program's main entry point.
///
/// main() will:
///    * make sure that the LOCALAPPDATA exists and has a directory in it called exifrensc
///    * see if settings.sqlite exists, if not create it by copying it from the resource stub
///    * launch our database server thread (which is an sqlite in memory database)
///    * initialise common controls
///    * launch our window  
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

    let mut test_studio: NxStudioDB = NxStudioDB { location: PathBuf::new(), success: false };

    if test_studio.existant() {
    } else {
        println!("No");
    }

    /*
     * Check to see if we have a directory set up in LOCALAPPDATA.
     * If we don't have it yet, then we will try to create it.
     */

    let mut my_appdata: String = env::var("LOCALAPPDATA").expect("$LOCALAPPDATA is not set.");
    my_appdata.push_str("\\exifrensc");
    let test_if_we_have_our_app_data_directory_set_up = PathBuf::from(&my_appdata);
    if !test_if_we_have_our_app_data_directory_set_up.is_dir() {
        if let Err(_e) = fs::create_dir_all(test_if_we_have_our_app_data_directory_set_up) {
            Fail!("Failed to create \"$LOCALAPPDATA\\exifrensc\".");
            panic!("Failed to create \"$LOCALAPPDATA\\exifrensc\".");
        }

        /*
         * One last check to see if the directory exists
         */

        if !PathBuf::from(&my_appdata).is_dir() {
            Fail!("Could not find and/or create \"$LOCALAPPDATA\\exifrensc\".");
            panic!("Still can not find $LOCALAPPDATA.");
        }
    }

    /*
     * Check to see if we already have a copy of the settings database.
     * If not, extract a copy from the resource stub.
     *
     * On this occasion I am saving my settings in an sqlite database rather than the registry.
     * This is in part for "proof of concept", but also exposes the settings to any sql scripts which may need them.
     */

    unsafe {
        path_to_settings_sqlite = my_appdata + ("\\settings.sqlite");
        if (!Path::new(&path_to_settings_sqlite).exists()) {
            ResourceSave(IDB_SETTINGS, "SQLITE\0", &path_to_settings_sqlite); // id: i32, section: &str, filename: &str

            if (!Path::new(&path_to_settings_sqlite).exists()) {
                FailU!("Could not create the settings file.");
                panic!("Still can not create the settings file");
            }
        }

        InitCommonControls();
        if let Ok(hinst) = GetModuleHandleA(None) {
            main_hwnd = CreateDialogParamA(hinst, PCSTR(IDD_MAIN as *mut u8), HWND(0), Some(main_dlg_proc), LPARAM(0));
            let mut message = MSG::default();

            let db_thread = thread::spawn(move || {
                mem_db();
            });

            while GetMessageA(&mut message, HWND(0), 0, 0).into() {
                if (IsDialogMessageA(main_hwnd, &message) == false) {
                    TranslateMessage(&message);
                    DispatchMessageA(&message);
                }
                if EXITERMINATE {
                    SendMessageA(main_hwnd, WM_COMMAND, WPARAM(2), LPARAM(0)); // push the cancel button in our main dialog
                }
            }
        }
        Ok(())
    }
}

/// Dialog callback function for our main window
extern "system" fn main_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    static mut segoe_mdl2_assets: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) }; // Has to be global because we need to destroy our font resource eventually

    unsafe {
        let hinst = GetModuleHandleA(None).unwrap();
        match nMsg {
            WM_INITDIALOG => {
                set_icon(hwnd);

                segoe_mdl2_assets.register_font(hwnd, s!("Segoe MDL2 Assets"), 16, FW_NORMAL.0, false);
                segoe_mdl2_assets.set_text(IDC_MAIN_ADD_PICTURE, w!("\u{EB9F}"), w!("Add photo(s)"));
                segoe_mdl2_assets.set_text(IDC_MAIN_ADD_FOLDER, w!("\u{ED25}"), w!("Add a folder full of photos"));
                segoe_mdl2_assets.set_text(IDC_MAIN_SAVE, w!("\u{E74E}"), w!("Save changes to names"));
                segoe_mdl2_assets.set_text(IDC_MAIN_RENAME, w!("\u{E8AC}"), w!("Manually rename selected photo"));
                segoe_mdl2_assets.set_text(IDC_MAIN_ERASE, w!("\u{ED60}"), w!("Remove selected photo from the list"));
                segoe_mdl2_assets.set_text(IDC_MAIN_DELETE, w!("\u{ED62}"), w!("Remove all photos from the list"));
                segoe_mdl2_assets.set_text(IDC_MAIN_INFO, w!("\u{E946}"), w!("About"));
                segoe_mdl2_assets.set_text(IDC_MAIN_SETTINGS, w!("\u{F8B0}"), w!("Settings"));
                segoe_mdl2_assets.set_text(IDC_MAIN_SYNC, w!("\u{EDAB}"), w!("Resync names"));

                //DragAcceptFiles(GetDlgItem(hwnd, IDC_MAIN_FILE_LIST) as HWND, true);

                /*
                 * If we wanted to set up a list box with files and directories, this is how we would do it.
                 * I've never much liked the Win 3.11 look to this function so don't use it.
                 *

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

                SendDlgItemMessageW(
                    hwnd,
                    IDC_MAIN_FILE_LIST,
                    LVM_SETEXTENDEDLISTVIEWSTYLE,
                    WPARAM((LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER).try_into().unwrap()),
                    LPARAM((LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER).try_into().unwrap()),
                );

                let mut lvC = LVCOLUMNA {
                    mask: LVCF_FMT | LVCF_TEXT | LVCF_SUBITEM | LVCF_WIDTH,
                    fmt: LVCFMT_LEFT,
                    cx: convert_x_to_client_coords(IDC_MAIN_FILE_LIST_R.width / 4),
                    pszText: transmute(utf8_to_utf16("Original File Name\0").as_ptr()),
                    cchTextMax: 0,
                    iSubItem: 0,
                    iImage: 0,
                    iOrder: 0,
                    cxMin: 50,
                    cxDefault: 55,
                    cxIdeal: 55,
                };

                SendDlgItemMessageW(hwnd, IDC_MAIN_FILE_LIST, LVM_INSERTCOLUMN, WPARAM(0), LPARAM(&lvC as *const _ as isize));

                lvC.iSubItem = 1;
                lvC.pszText = transmute(utf8_to_utf16("Changed File Name\0").as_ptr());
                SendDlgItemMessageW(hwnd, IDC_MAIN_FILE_LIST, LVM_INSERTCOLUMN, WPARAM(1), LPARAM(&lvC as *const _ as isize));

                lvC.pszText = transmute(utf8_to_utf16("File Created Time\0").as_ptr());
                SendDlgItemMessageW(hwnd, IDC_MAIN_FILE_LIST, LVM_INSERTCOLUMN, WPARAM(2), LPARAM(&lvC as *const _ as isize));

                lvC.pszText = transmute(utf8_to_utf16("Photo Taken Time\0").as_ptr());
                SendDlgItemMessageW(hwnd, IDC_MAIN_FILE_LIST, LVM_INSERTCOLUMN, WPARAM(3), LPARAM(&lvC as *const _ as isize));

                SendDlgItemMessageA(hwnd, IDC_MAIN_PATTERN, EM_SETLIMITTEXT, WPARAM(32), LPARAM(0));

                0
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam); // I am sure there has to be a better way to do this, but the only way I could get the value out of a WPARAM type was to transmute it to a u64
                wParam = (wParam << 48 >> 48); // LOWORD isn't defined, at least as far as I could tell, so I had to improvise

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL {
                    segoe_mdl2_assets.destroy();
                    PostQuitMessage(0);
                } else {
                    match wParam as i32 {
                        IDC_MAIN_ADD_PICTURE => {
                            LoadFile();
                        }
                        IDC_MAIN_ADD_FOLDER => {
                            LoadDirectory();
                        }
                        IDC_MAIN_SAVE => {
                            LoadFile();
                        }
                        IDC_MAIN_DELETE => {
                            LoadFile();
                        }
                        IDC_MAIN_ERASE => {
                            // let o = minreq::get(HOST_URL.to_owned() + "/aero?planejellyfor me").with_header("X-Bonafide", BONAFIDE.as_str()).send().expect("minreq send failed");
                            // let s = o.as_str().unwrap();
                        }
                        IDC_MAIN_SYNC => {
                            LoadFile();
                        }
                        IDC_MAIN_SETTINGS => {
                            CreateDialogParamA(hinst, PCSTR(IDD_SETTINGS as *mut u8), HWND(0), Some(settings_dlg_proc), LPARAM(0));
                        }

                        IDC_MAIN_INFO => {
                            CreateDialogParamA(hinst, PCSTR(IDD_ABOUT as *mut u8), HWND(0), Some(about_dlg_proc), LPARAM(0));
                        }
                        _ => {}
                    }
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
                //     SetWindowPos( GetDlgItem(hwnd, IDC_MAIN_FILE_LIST) as HWND, HWND_TOP,
                //                   borrowed_rect.left,borrowed_rect.top,
                //                   borrowed_rect.right-borrowed_rect.left,borrowed_rect.bottom-borrowed_rect.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                //     }

                // Because that didn't work as advertised, perhaps because I am using Segoe UI as the font instead of the default font,
                // which is MS Shell Dialog and dates back to XP (or earlier?), I calculate the resizing manually based on Segoe UI.
                // I am not sure what effects this might have on other monitors with different resolutions of DPI settings.

                SetWindowPos(
                    GetDlgItem(hwnd, IDC_MAIN_FILE_LIST_R.id) as HWND,
                    HWND_TOP,
                    convert_x_to_client_coords(IDC_MAIN_FILE_LIST_R.x),
                    convert_y_to_client_coords(IDC_MAIN_FILE_LIST_R.y),
                    new_width - convert_x_to_client_coords(IDC_MAIN_FILE_LIST_R.x + 8),
                    new_height - convert_y_to_client_coords(IDC_MAIN_FILE_LIST_R.y + 8),
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                SetWindowPos(
                    GetDlgItem(hwnd, IDC_MAIN_PATTERN_R.id) as HWND,
                    HWND_TOP,
                    convert_x_to_client_coords(IDC_MAIN_PATTERN_R.x),
                    convert_y_to_client_coords(IDC_MAIN_PATTERN_R.y),
                    new_width - convert_x_to_client_coords(IDC_MAIN_PATTERN_R.x + 26),
                    convert_y_to_client_coords(IDC_MAIN_PATTERN_R.height),
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                SetWindowPos(
                    GetDlgItem(hwnd, IDC_MAIN_SYNC_R.id) as HWND,
                    HWND_TOP,
                    new_width - convert_x_to_client_coords(23),
                    convert_y_to_client_coords(IDC_MAIN_PATTERN_R.y - 1),
                    convert_x_to_client_coords(IDC_MAIN_SYNC_R.width),
                    convert_y_to_client_coords(IDC_MAIN_SYNC_R.height),
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                0
            }

            WM_DROPFILES => {
                let mut file_name_buffer = [0; MAX_PATH as usize];
                let hDrop: HDROP = HDROP(transmute(wParam));
                let nFiles: u32 = DragQueryFileA(hDrop, 0xFFFFFFFF, Some(file_name_buffer.as_mut_slice())); // Wish I could send a NULL as the last param since I don't really need to pass a buffer for this call

                /*
                 * We will just run a "protection" flag over any current files which are in our database
                 * to ensure they do not get deleted in the last step which is removing any files dropped
                 * into the database which are not images.
                 */

                QuickNonReturningSqlCommand("UPDATE files SET tmp_lock=1;".to_string());

                for i in 0..nFiles
                // Walk through the dropped "files" one by one, but they may not all be files, some may be directories 😛
                {
                    DragQueryFileA(hDrop, i, Some(file_name_buffer.as_mut_slice()));
                    let mut file_path = String::from_utf8_unchecked(file_name_buffer.to_vec());
                    file_path.truncate(file_path.find('\0').unwrap());

                    let test_Path = PathBuf::from(&file_path);
                    if test_Path.is_dir() {
                        WalkDirectoryAndAddFiles(&test_Path);
                    } else {
                        check_and_add_this_file_to_the_database(file_path);
                    }
                }

                delete_unwanted_files_after_bulk_import();
                DragFinish(hDrop);
                0
            }

            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => 0,
        }
    }
}

/// Dialog callback for our settings window
extern "system" fn settings_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    static mut segoe_mdl2_assets: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) }; // Has to be global because we need to destroy our font resource eventually
    unsafe {
        let hinst = GetModuleHandleA(None).unwrap();
        match nMsg {
            WM_INITDIALOG => {
                set_icon(hwnd);

                /*
                 * Set up our action buttons
                 */
                segoe_mdl2_assets.register_font(hwnd, s!("Segoe MDL2 Assets"), 14, FW_NORMAL.0, false);
                segoe_mdl2_assets.set_text(IDC_PREFSAddAMask, w!("\u{E710}"), w!("Add new file pattern"));
                segoe_mdl2_assets.set_text(IDC_PREFSDelPattern, w!("\u{E74D}"), w!("Delete file pattern"));

                /*
                 * Set up our combo boxes
                 */
                SendDlgItemMessageW(hwnd, IDC_PREFS_ON_CONFLICT, CB_ADDSTRING, WPARAM(0), LPARAM(w!("Add\0").as_ptr() as isize));
                SendDlgItemMessageW(hwnd, IDC_PREFS_ON_CONFLICT, CB_ADDSTRING, WPARAM(0), LPARAM(w!("Skip\0").as_ptr() as isize));
                SendDlgItemMessageA(hwnd, IDC_PREFS_ON_CONFLICT, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_ON_CONFLICT)), LPARAM(0));

                let dlgIDC_PREFS_ON_CONFLICT_ADD: HWND = GetDlgItem(hwnd, IDC_PREFS_ON_CONFLICT_ADD);
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_ADD, CB_ADDSTRING, WPARAM(0), LPARAM(w!("_\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_ADD, CB_ADDSTRING, WPARAM(0), LPARAM(w!("-\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_ADD, CB_ADDSTRING, WPARAM(0), LPARAM(w!(".\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_ADD, CB_ADDSTRING, WPARAM(0), LPARAM(w!("~\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_ADD, CB_ADDSTRING, WPARAM(0), LPARAM(w!("No delimeter\0").as_ptr() as isize));
                SendMessageA(dlgIDC_PREFS_ON_CONFLICT_ADD, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_ON_CONFLICT_ADD)), LPARAM(0));

                let dlgIDC_PREFS_ON_CONFLICT_NUM: HWND = GetDlgItem(hwnd, IDC_PREFS_ON_CONFLICT_NUM);
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_NUM, CB_ADDSTRING, WPARAM(0), LPARAM(w!("12345\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_NUM, CB_ADDSTRING, WPARAM(0), LPARAM(w!("1\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_NUM, CB_ADDSTRING, WPARAM(0), LPARAM(w!("02\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_NUM, CB_ADDSTRING, WPARAM(0), LPARAM(w!("003\0").as_ptr() as isize));
                SendMessageA(dlgIDC_PREFS_ON_CONFLICT_NUM, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_ON_CONFLICT_NUM)), LPARAM(0));

                let dlgIDC_PREFS_DATE_SHOOT_PRIMARY: HWND = GetDlgItem(hwnd, IDC_PREFS_DATE_SHOOT_PRIMARY);
                SendMessageW(dlgIDC_PREFS_DATE_SHOOT_PRIMARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("the date shot in the EXIF data\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_DATE_SHOOT_PRIMARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("use \"File Created\" date\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_DATE_SHOOT_PRIMARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("use \"Last Modified\" date\0").as_ptr() as isize));
                SendMessageA(dlgIDC_PREFS_DATE_SHOOT_PRIMARY, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_DATE_SHOOT_PRIMARY)), LPARAM(0));

                SendDlgItemMessageW(hwnd, IDC_PREFS_DATE_SHOOT_SECONDARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("use \"File Created\" date\0").as_ptr() as isize));
                SendDlgItemMessageW(hwnd, IDC_PREFS_DATE_SHOOT_SECONDARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("use \"Last Modified\" date\0").as_ptr() as isize));
                SendDlgItemMessageA(hwnd, IDC_PREFS_DATE_SHOOT_SECONDARY, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_DATE_SHOOT_SECONDARY)), LPARAM(0));

                /*
                 * Setup up the file mask box, which is a listview
                 * Kind of in parrallel we will also set up the drag and drop filter box at the same time
                 */
                let dlgFileMask: HWND = GetDlgItem(hwnd, IDC_PREFS_FILE_MASK);
                let dlgIDC_IDC_PREFS_DRAG_N_DROP: HWND = GetDlgItem(hwnd, IDC_PREFS_DRAG_N_DROP);

                SendMessageW(
                    dlgFileMask,
                    LVM_SETEXTENDEDLISTVIEWSTYLE,
                    WPARAM((LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER).try_into().unwrap()),
                    LPARAM((LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER).try_into().unwrap()),
                );

                let mut lvC = LVCOLUMNA {
                    mask: LVCF_FMT | LVCF_TEXT | LVCF_SUBITEM | LVCF_WIDTH,
                    fmt: LVCFMT_LEFT,
                    cx: convert_x_to_client_coords(IDC_PREFS_FILE_MASK_R.width - 12) / 2,
                    pszText: transmute(w!("Pattern description").as_ptr()),
                    cchTextMax: 0,
                    iSubItem: 0,
                    iImage: 0,
                    iOrder: 0,
                    cxMin: 50,
                    cxDefault: 55,
                    cxIdeal: 55,
                };

                SendMessageW(dlgFileMask, LVM_INSERTCOLUMN, WPARAM(0), LPARAM(&lvC as *const _ as isize));

                lvC.iSubItem = 1;
                lvC.pszText = transmute(w!("File pattern/mask").as_ptr());
                SendMessageW(dlgFileMask, LVM_INSERTCOLUMN, WPARAM(1), LPARAM(&lvC as *const _ as isize));

                let mut fileNames: Vec<Vec<u16>> = Vec::new(); // File name pointers
                let mut fileSpecs: Vec<Vec<u16>> = Vec::new(); // File Spec pointers

                // Ask out database how many predefined file patterns there are
                for i in 0..Count("idx", "file_pat") {
                    if i > 15 {
                        let _x_ = MessageBoxA(None, s!("Sorry, but there is unfortunately a hard limit of 15 file patterns."), s!("Settings"), MB_OK | MB_ICONEXCLAMATION);
                        // We only accept 16 file masks (at this time), so we jump out if we hit that limit
                        break;
                    }

                    let mut Name: String = String::new();
                    let mut Spec: String = String::new();

                    GetFilePatterns(i + 1, &mut Name, &mut Spec); // retrieve from the database the values

                    // We need a null terminator on the string for windows
                    Name.push('\0');
                    Spec.push('\0');

                    // Copy the wildcard pattern into our dropdown
                    SendMessageW(dlgIDC_IDC_PREFS_DRAG_N_DROP, CB_ADDSTRING, WPARAM(0), LPARAM(utf8_to_utf16(&Spec).as_ptr() as isize));

                    // Convert the UTF8 to UTF16 (for windows) and push into a vector to keep it alive for a while
                    fileNames.push(utf8_to_utf16(&Name));
                    fileSpecs.push(utf8_to_utf16(&Spec));

                    let iColFmt: u32 = 0;
                    let uColumns: i32 = 0;
                    let mut lv = LVITEMW {
                        mask: LVIF_TEXT,
                        iItem: i.try_into().unwrap(),
                        iSubItem: 0,
                        state: LIST_VIEW_ITEM_STATE_FLAGS(0),
                        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
                        pszText: transmute(fileNames[i].as_ptr()),
                        cchTextMax: 0,
                        iImage: 0,
                        lParam: LPARAM(0),
                        iIndent: 0,
                        iGroupId: LVITEMA_GROUP_ID(0),
                        cColumns: 0,
                        puColumns: transmute(&uColumns),
                        piColFmt: transmute(&iColFmt),
                        iGroup: 0,
                    };

                    SendMessageW(dlgFileMask, LVM_INSERTITEM, WPARAM(0), LPARAM(&lv as *const _ as isize));
                    lv.pszText = transmute(fileSpecs[i].as_ptr());
                    lv.iSubItem = 1;
                    SendMessageW(dlgFileMask, LVM_SETITEMTEXT, WPARAM(i), LPARAM(&lv as *const _ as isize));
                }

                SendMessageA(dlgIDC_IDC_PREFS_DRAG_N_DROP, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_DRAG_N_DROP)), LPARAM(0));

                /*
                 * Copy the file pattern database into a temporary location so we can facilitate cancel/undo
                 */

                MakeTempFilePatternDatabase();

                /*
                 * Check to see if NX Studio is installed, and if it is, see if we can find the database file
                 * If we can not then we will disable getting to choose to use it as an option.
                 */
                let mut NX_Studio: NxStudioDB = NxStudioDB { location: PathBuf::new(), success: false };

                let NX_stu_DlgItem: HWND = GetDlgItem(hwnd, IDC_PREFS_NX_STUDIO);

                if !NX_Studio.existant() {
                    EnableWindow(NX_stu_DlgItem, false);
                    SendMessageA(NX_stu_DlgItem, BM_SETCHECK, WPARAM(BST_UNCHECKED.0.try_into().unwrap()), LPARAM(0));
                } else if GetIntSetting(IDC_PREFS_NX_STUDIO) == 1 {
                    SendMessageA(NX_stu_DlgItem, BM_SETCHECK, WPARAM(BST_CHECKED.0.try_into().unwrap()), LPARAM(0));
                } else {
                    SendMessageA(NX_stu_DlgItem, BM_SETCHECK, WPARAM(BST_UNCHECKED.0.try_into().unwrap()), LPARAM(0));
                }
                0
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam);
                wParam = (wParam << 48 >> 48); // LOWORD

                match wParam as i32 {
                    IDC_PREFS_CANCEL | ID_CANCEL => {
                        RestoreFilePatternDatabase();
                        segoe_mdl2_assets.destroy();
                        EndDialog(hwnd, 0);
                    }
                    IDC_PREFS_APPLY => {
                        ApplySettings(hwnd);
                        segoe_mdl2_assets.destroy();
                        EndDialog(hwnd, 0);
                    }
                    IDC_PREFS_SAVE_SETTING => {
                        ApplySettings(hwnd);
                        SaveSettings();
                        segoe_mdl2_assets.destroy();
                        EndDialog(hwnd, 0);
                    }
                    IDC_PREFS_RESET_SETTING => {
                        /* To "reset" all we do is write over the top of the settings file in the local app directory
                         * with the default settings file, which is saved in the resource stub.
                         */
                        if MessageBoxA(None, s!("Are you sure you want to reset the settings?"), s!("I want to know!"), MB_YESNO | MB_ICONEXCLAMATION) == IDYES {
                            ResourceSave(IDB_SETTINGS, "SQLITE\0", &path_to_settings_sqlite);
                            ReloadSettings();
                            segoe_mdl2_assets.destroy();
                            EndDialog(hwnd, 0);
                        }
                    }

                    IDM_PrefsFileMaskDel | IDC_PREFSDelPattern => {
                        let dlgFileMask: HWND = GetDlgItem(hwnd, IDC_PREFS_FILE_MASK);
                        if SendMessageA(dlgFileMask, LVM_GETSELECTEDCOUNT, WPARAM(0), LPARAM(0)) != LRESULT(0) {
                            let selected = SendMessageA(dlgFileMask, LVM_GETSELECTIONMARK, WPARAM(0), LPARAM(0));
                            let mut name_buffer = [0; 128_usize];
                            let lv = LVITEMW {
                                mask: LVIF_TEXT,
                                iItem: 0,
                                iSubItem: 0,
                                state: LIST_VIEW_ITEM_STATE_FLAGS(0),
                                stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
                                pszText: transmute(name_buffer.as_ptr()),
                                cchTextMax: 128,
                                iImage: 0,
                                lParam: LPARAM(0),
                                iIndent: 0,
                                iGroupId: LVITEMA_GROUP_ID(0),
                                cColumns: 0,
                                puColumns: std::ptr::null_mut(),
                                piColFmt: std::ptr::null_mut(),
                                iGroup: 0,
                            };

                            SendMessageA(dlgFileMask, LVM_GETITEMTEXT, WPARAM(selected.0.try_into().unwrap()), LPARAM(&lv as *const _ as isize));

                            let mut utf7_buffer: [u8; 64] = [0; 64_usize];
                            let mut i = 0;
                            let mut j = 0;

                            /*
                             * Convert to ASCII/UTF7 (kind of 🙄)
                             * We do this in a super dodgy way - just take every second character
                             * and copy it into a new buffer, getting rid of the utf16 bit,
                             * then we make a utf8 string out of it, and truncate it on the
                             * first null character. We probably should check that every
                             * second character is in fact a null, but in this context I am
                             * confident that they are.
                             *
                             */
                            while name_buffer[i] != 0 {
                                utf7_buffer[j] = name_buffer[i];
                                i += 2;
                                j += 1;
                            }
                            let mut name = String::from_utf8_unchecked(utf7_buffer.to_vec());
                            name.truncate(name.find('\0').unwrap());

                            if name == "All files" {
                                let _x_ = MessageBoxA(None, s!("Sorry, but that one has to stay."), s!("Delete File Pattern"), MB_OK | MB_ICONEXCLAMATION);
                            } else if MessageBoxA(None, s!("Are you sure you want to delete this?"), s!("Delete File Pattern"), MB_YESNO | MB_ICONEXCLAMATION) == IDYES {
                                DeleteFilePattern(&mut name);
                                SendMessageA(dlgFileMask, LVM_DELETEITEM, WPARAM(selected.0.try_into().unwrap()), LPARAM(0));
                            }
                        }
                    }
                    IDM_PrefsFileMaskAdd | IDC_PREFSAddAMask => {
                        let selected = SendMessageA(GetDlgItem(hwnd, IDC_PREFS_FILE_MASK), LVM_GETSELECTIONMARK, WPARAM(0), LPARAM(0));
                        CreateDialogParamA(hinst, PCSTR(IDD_ADD_FILE_MASK as *mut u8), hwnd, Some(add_file_mask_dlg_proc), LPARAM(selected.0));
                    }
                    _ => {}
                }
                0
            }
            /*             WM_CONTEXTMENU =>{
                           println!("WM_CONTEXTMENU");
                           0
                       }
            */
            WM_NOTIFY => {
                if (lParamTOnmhdr(transmute(lParam)).0 == IDC_PREFS_FILE_MASK) && (lParamTOnmhdr(transmute(lParam)).1 == NM_RCLICK) {
                    /*
                     * Setup our right-click context menu
                     */

                    let mut xy = POINT { x: 0, y: 0 };

                    /*
                     * We will load the menu from the resource file, but the next two lines show how to do it inline:
                     * let mut myPopup: HMENU = CreatePopupMenu().unwrap();
                     * InsertMenuA(myPopup, 0, MF_BYCOMMAND | MF_STRING | MF_ENABLED, 1, s!("Hello"));
                     */
                    let rootmenu: HMENU = LoadMenuW(hinst, PCWSTR(IDR_PrefsFileMask as *mut u16)).unwrap();
                    let myPopup: HMENU = GetSubMenu(rootmenu, 0);
                    GetCursorPos(&mut xy);
                    TrackPopupMenu(myPopup, TPM_TOPALIGN | TPM_LEFTALIGN, xy.x, xy.y, 0, hwnd, None);
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

/// Dialog callback for our add a new file mask dialog
//
extern "system" fn add_file_mask_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    static mut selected_: LPARAM = LPARAM(0);
    unsafe {
        match nMsg {
            WM_INITDIALOG => {
                set_icon(hwnd);
                SendDlgItemMessageA(hwnd, IDC_AddPatDescription, EM_SETLIMITTEXT, WPARAM(32), LPARAM(0));
                SendDlgItemMessageA(hwnd, IDC_AddFileMaskFileMask, EM_SETLIMITTEXT, WPARAM(32), LPARAM(0));
                selected_ = lParam;

                0
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam);
                wParam = (wParam << 48 >> 48); // LOWORD

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL {
                    EndDialog(hwnd, 0);
                    //
                } else if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDOK {
                    let settings_hwnd: HWND = GetParent(hwnd); // Have to find the settings window this sneaky way because we used lParam to pass the selected item

                    // Get the text out of the two input boxes
                    let mut text: [u16; 64] = [0; 64];
                    let len = GetWindowTextW(GetDlgItem(hwnd, IDC_AddPatDescription), &mut text);
                    let mut patDescription = String::from_utf16_lossy(&text[..len as usize]);
                    patDescription.push('\0');
                    let len = GetWindowTextW(GetDlgItem(hwnd, IDC_AddFileMaskFileMask), &mut text);
                    let mut fileMask = String::from_utf16_lossy(&text[..len as usize]);
                    fileMask.push('\0');

                    // Insert the new values into the listview in the settings window
                    let dlgFileMask: HWND = GetDlgItem(settings_hwnd, IDC_PREFS_FILE_MASK);
                    let iColFmt: u32 = 0;
                    let uColumns: i32 = 0;
                    let mut lv = LVITEMW {
                        mask: LVIF_TEXT,
                        iItem: selected_.0.try_into().unwrap(),
                        iSubItem: 0,
                        state: LIST_VIEW_ITEM_STATE_FLAGS(0),
                        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
                        pszText: transmute(utf8_to_utf16(&patDescription).as_ptr()),
                        cchTextMax: 0,
                        iImage: 0,
                        lParam: LPARAM(0),
                        iIndent: 0,
                        iGroupId: LVITEMA_GROUP_ID(0),
                        cColumns: 0,
                        puColumns: transmute(&uColumns),
                        piColFmt: transmute(&iColFmt),
                        iGroup: 0,
                    };

                    SendMessageW(dlgFileMask, LVM_INSERTITEM, WPARAM(0), LPARAM(&lv as *const _ as isize));
                    lv.pszText = transmute(utf8_to_utf16(&fileMask).as_ptr());
                    lv.iSubItem = 1;
                    SendMessageW(dlgFileMask, LVM_SETITEMTEXT, WPARAM(selected_.0.try_into().unwrap()), LPARAM(&lv as *const _ as isize));

                    AddFilePattern(selected_.0.try_into().unwrap(), patDescription, fileMask);

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

/// Dialog callback for our about window
///
/// Mostly this is just changing fonts
extern "system" fn about_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, _lParam: LPARAM) -> isize {
    // Have to be global because we need to destroy our font resources eventually
    static mut segoe_bold_9: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) };
    static mut segoe_bold_italic_13: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) };
    static mut segoe_italic_10: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) };

    unsafe {
        match nMsg {
            WM_INITDIALOG => {
                set_icon(hwnd);

                let annaversionary = chrono::Local.ymd(2022, 6, 17).and_hms(0, 0, 0);
                let majorversion = env!("CARGO_PKG_VERSION_MAJOR");
                let minorversion = env!("CARGO_PKG_VERSION_MINOR");
                let now = Local::now();
                let diff = now.signed_duration_since(annaversionary);
                let days = diff.num_days();
                let minutes = (diff.num_seconds() - (days * 86400)) / 60;
                let iso_8601 = now.format("%Y-%m-%d %H:%M\0").to_string();
                let vers = format!("{}.{}.{}.{}\0", majorversion, minorversion, days, minutes);
                let copyright: String = now.format("2022-%Y\0").to_string();

                segoe_bold_9.register_font(hwnd, s!("Segoe UI"), 9, FW_BOLD.0, false);
                segoe_bold_9.set_font(IDC_ABOUT_ST_VER);
                segoe_bold_9.set_font(IDC_ABOUT_BUILT);
                segoe_bold_9.set_font(IDC_ABOUT_ST_AUTHOR);
                segoe_bold_9.set_font(IDC_ABOUT_ST_COPY);

                segoe_bold_italic_13.register_font(hwnd, s!("Segoe UI"), 13, FW_BOLD.0, true);
                segoe_bold_italic_13.set_font(IDC_ABOUT_TITLE);

                segoe_italic_10.register_font(hwnd, s!("Segoe UI"), 10, FW_NORMAL.0, true);
                segoe_italic_10.set_font(IDC_ABOUT_DESCRIPTION);

                SetDlgItemTextA(hwnd, IDC_ABOUT_VERSION, PCSTR(vers.as_ptr()));
                SetDlgItemTextA(hwnd, IDC_ABOUT_BUILDDATE, PCSTR(iso_8601.as_ptr()));
                SetDlgItemTextA(hwnd, IDC_COPYRIGHT, PCSTR(copyright.as_ptr()));

                0
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam);
                wParam = (wParam << 48 >> 48); // LOWORD

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

/// Set our dialog/windows icon to the program's default
fn set_icon(hwnd: HWND) {
    unsafe {
        let hinst = GetModuleHandleA(None).unwrap();
        let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
        SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon.unwrap().0));

        let icon = LoadIconW(hinst, PCWSTR(IDI_PROG_ICON as *mut u16));
        SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(icon.unwrap().0));
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

/// Extract the dialog ID and message from the NMHDR structure returned in lParam
fn lParamTOnmhdr(nmhdr: *const NMHDR) -> (i32, u32) {
    unsafe { ((*nmhdr).idFrom.try_into().unwrap(), (*nmhdr).code) }
}

struct WindowsControlText {
    hwnd: HWND,
    hfont: HFONT,
}

impl WindowsControlText {
    /**
     * Register a font and size
     **/
    fn register_font(&mut self, hwnd: HWND, face: PCSTR, pitch: i32, weight: u32, italic: bool) {
        unsafe {
            let hdc = GetDC(hwnd);
            self.hfont = CreateFontA(
                (-pitch * GetDeviceCaps(hdc, LOGPIXELSY)) / 72, // logical height of font
                0,                                              // logical average character width
                0,                                              // angle of escapement
                0,                                              // base-line orientation angle
                weight.try_into().unwrap(),                     // font weight
                italic as u32,                                  // italic attribute flag
                0,                                              // underline attribute flag
                0,                                              // strikeout attribute flag
                ANSI_CHARSET.0.into(),                          // character set identifier
                OUT_DEFAULT_PRECIS.0.into(),                    // output precision
                CLIP_DEFAULT_PRECIS.0.into(),                   // clipping precision
                PROOF_QUALITY.0.into(),                         // output quality
                FF_DECORATIVE.0.into(),                         // pitch and family
                face,                                           // pointer to typeface name string
            );
            self.hwnd = hwnd;
            ReleaseDC(hwnd, hdc);
        }
    }

    /**
     * Set the caption and tool tip text of a windows control.
     **/
    fn set_text(&self, id: i32, caption: PCWSTR, tooltip_text: PCWSTR) {
        unsafe {
            let hinst = GetModuleHandleA(None).unwrap();

            SendDlgItemMessageA(self.hwnd, id, WM_SETFONT, WPARAM(self.hfont.0 as usize), LPARAM(0));

            if caption != w!("") {
                SetDlgItemTextW(self.hwnd, id, caption);
            }

            if tooltip_text != w!("") {
                let tt_hwnd = CreateWindowExA(
                    Default::default(),
                    PCSTR("tooltips_class32\0".as_ptr()), // Have to add a trailling NULL or this call wont work since Rust does't typicaally add NULLs but windows likes them
                    None,
                    WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP), // | WINDOW_STYLE(TTS_BALLOON), // I don't really like the balloon style, but this is how we'd define it
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    self.hwnd,
                    None,
                    hinst,
                    None,
                );

                let toolInfo = TTTOOLINFOA {
                    cbSize: mem::size_of::<TTTOOLINFOA>() as u32,
                    uFlags: TTF_IDISHWND | TTF_SUBCLASS,
                    hwnd: self.hwnd,                                     // Handle to the hwnd that contains the tool
                    uId: transmute(GetDlgItem(self.hwnd, id)),           // hwnd handle to the tool. or parent_hwnd
                    rect: RECT { left: 0, top: 0, right: 0, bottom: 0 }, // bounding rectangle coordinates of the tool, don't use, but seems to need to supply to stop it grumbling
                    hinst,                                               // Our hinstance
                    lpszText: transmute(tooltip_text.as_ptr()),          // Pointer to a utf16 buffer with the tooltip text
                    lParam: LPARAM(id.try_into().unwrap()),              // A 32-bit application-defined value that is associated with the tool
                    lpReserved: std::ptr::null_mut::<c_void>(),          // Reserved. Must be set to NULL
                };

                SendMessageA(tt_hwnd, TTM_ADDTOOL, WPARAM(0), LPARAM(&toolInfo as *const _ as isize));
                SendMessageA(tt_hwnd, TTM_SETMAXTIPWIDTH, WPARAM(0), LPARAM(200));
            }
        }
    }

    fn set_font(&self, id: i32) {
        unsafe {
            let hinst = GetModuleHandleA(None).unwrap();
            SendDlgItemMessageA(self.hwnd, id, WM_SETFONT, WPARAM(self.hfont.0 as usize), LPARAM(0));
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
///
/// Possibly redundant now we have the !w macro which seems to do much the same thing?
/// Actually, not - can still be used on content which isn't known at compile time,
/// whereas w! and !s are macros executed at compile time so can't be used with dynamic content.
fn utf8_to_utf16(utf8_in: &str) -> Vec<u16> {
    utf8_in.encode_utf16().collect()
}

//fn LoadFile() -> Result<()> {
fn LoadFile() {
    unsafe {
        let file_dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).unwrap();

        // Change a few of the default options for the dialog
        file_dialog.SetTitle(w!("Choose Photos to Rename")).expect("SetTitle() failed in LoadFile()");
        file_dialog.SetOkButtonLabel(w!("Select Photos")).expect("SetOkButtonLabel() failed in LoadFile()");

        /*
         * Next we are going to set up the file types combo box for the file selection dialog.
         * This is not as simple as it seems. Firstly we have to create an array of blank
         * COMDLG_FILTERSPEC structures, we make 16 in total. Following this we will ask
         * our in memory database to give us the file name and its specs. These have to be
         * converted from ASCII to UTF16, and the UTF 16 is stored in a vevtor of u16.
         * But we need a vector of u16 vectors to keep the value from being destroyed long
         * enough for the dialog to initialise.
         * You have no idea how long this actually took to figure out. Its kind of
         * embarassing even thought the solution was quite simple in the end.
         */
        let mut file_pat: [COMDLG_FILTERSPEC; 16] = [
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
            COMDLG_FILTERSPEC { pszName: w!(""), pszSpec: w!("") },
        ];

        let mut fileNames: Vec<Vec<u16>> = Vec::new(); // File name pointers
        let mut fileSpecs: Vec<Vec<u16>> = Vec::new(); // File Spec pointers

        for i in 0..Count("idx", "file_pat")
        // Ask our database how many predefined file patterns there are
        {
            if i > 15 {
                let _x_ = MessageBoxA(None, s!("Sorry, but there is unfortunately a hard limit of 15 file patterns."), s!("Load File"), MB_OK | MB_ICONEXCLAMATION);
                // We only accept 16 file masks (at this time), so we jump out if we hit that limit
                break;
            }

            let mut Name: String = String::new();
            let mut Spec: String = String::new();

            GetFilePatterns(i + 1, &mut Name, &mut Spec); // retrieve from the database the values

            // We need a null terminator on the string for windows
            Name.push('\0');
            Spec.push('\0');

            // Convert the UTF8 to UTF16 (for windows) and push into a vector to keep it alive for a while
            fileNames.push(utf8_to_utf16(&Name));
            fileSpecs.push(utf8_to_utf16(&Spec));

            // Finally populate our COMDLG_FILTERSPEC structure
            file_pat[i].pszName = transmute(fileNames[i].as_ptr());
            file_pat[i].pszSpec = transmute(fileSpecs[i].as_ptr());
        }

        file_dialog.SetFileTypes(&file_pat).unwrap();
        let x: u32 = GetIntSetting(IDC_PREFS_DRAG_N_DROP).try_into().unwrap();
        file_dialog.SetFileTypeIndex(x + 1).unwrap();

        /* Don't know why this does not work! 😪
        let defPath: IShellItem = SHCreateItemInKnownFolder(&FOLDERID_Pictures, KF_FLAG_DEFAULT.0.try_into().unwrap(), None).unwrap();
                file_dialog.SetDefaultFolder(&defPath);
         */
        let mut options = file_dialog.GetOptions().unwrap();
        options.0 |= FOS_ALLOWMULTISELECT.0;
        file_dialog.SetOptions(options).expect("SetOptions() failed in LoadFile()");

        let answer = file_dialog.Show(None); // Basically an error means no file was selected

        /*  Single file select version

            if let Ok(__dummy) = answer {
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

        // Multi-selection version
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
                let file_name_s = String::from_utf16(tmp_file_name).unwrap();
                check_and_add_this_file_to_the_database(file_name_s);

                CoTaskMemFree(Some(transmute(file_name.0))); // feel rather nervy about this - not sure this is trying to free the right thing
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
        file_dialog.SetTitle(w!("Choose Directories of Photos to Add")).expect("SetTitle() failed in LoadDirectory()");
        file_dialog.SetOkButtonLabel(w!("Select Directories")).expect("SetOkButtonLabel() failed in LoadDirectory()");
        let mut options = file_dialog.GetOptions().unwrap();
        options.0 = options.0 | FOS_PICKFOLDERS.0 | FOS_ALLOWMULTISELECT.0;
        file_dialog.SetOptions(options).expect("SetOptions() failed in LoadDirectory()");

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
            let directory_name_s = String::from_utf16(tmp_directory_name).unwrap(); // convert our utf16 buffer to a rust string
            QuickNonReturningSqlCommand("UPDATE files SET tmp_lock=1;".to_string());
            WalkDirectoryAndAddFiles(&PathBuf::from(directory_name_s));
            delete_unwanted_files_after_bulk_import();
            CoTaskMemFree(Some(transmute(directory_name.0)));
        }

        //file_dialog.Release();
    }
    //    Ok(())
}

/// Takes a file path, then sees if there is a Nikon sidecar file which matches it. If there is, returns the path
/// to the sidecar file as a String, if not return a blank String.
fn get_nksc_file_path(file_to_check: &PathBuf) -> String {
    let nksc_param_path = file_to_check.parent().unwrap().to_path_buf().join("NKSC_PARAM").join(file_to_check.file_name().unwrap());
    let nksc_path = format!("{}.nksc", nksc_param_path.as_path().display());
    let nksc_path_to_test = Path::new(&nksc_path);
    if nksc_path_to_test.is_file() {
        nksc_path
    } else {
        "".to_string()
    }
}

/// Walks a directory looking for files and adding them to our in memory databse
///
/// Function makes two passes, the first time looking for the Nikon params directory, from which it will grab a copy internally
/// so it can map out where the corrosponding entry is, then it looks for the files.
fn WalkDirectoryAndAddFiles(WhichDirectory: &PathBuf) {
    if WhichDirectory.is_dir()
    // sanity check, probably not necessary, but this is Rust and Rust is all about "safety"
    {
        let nksc_param_path = WhichDirectory.clone().join("NKSC_PARAM");
        let mut nksc_param_paths = HashMap::new();
        let mut nksc_path = String::new();
        let mut nksc_name = String::new();

        /*
         * First look for the sidecar directory, nksc, then populate our HasMap with the key,
         * which is just the equivalent .nef name, and the value is the path to the sidecar file.
         */
        if nksc_param_path.exists() && nksc_param_path.is_dir() {
            let paths = fs::read_dir(nksc_param_path).expect("Could not scan the NIKON_PARAM directory 😥.");
            for each_path in paths {
                let file_path = each_path.unwrap();

                if file_path.path().is_file() && file_path.path().extension() == Some(OsStr::new("nksc")) {
                    nksc_path = format!("{}", file_path.path().display());
                    let file_delimeter = nksc_path.rfind('\\').unwrap();
                    let last_extension_delimeter = nksc_path.rfind('.').unwrap();
                    nksc_name = nksc_path.trim()[file_delimeter + 1..last_extension_delimeter].to_string();

                    nksc_param_paths.insert(nksc_name, nksc_path);
                }
            }
        }

        /*
         * Now we will look for the files in the directory we just dropped, check to see if there is an associated
         * sidecar file, then add them to our in memory database.
         */
        let paths = fs::read_dir(WhichDirectory).expect("Could not scan the directory 😥.");
        for each_path in paths {
            let file_path = each_path.unwrap();

            if (file_path.path().is_file()) {
                let created_datetime = get_file_created_timestamp_as_iso8601(&file_path.path());

                /*
                 * Get the file name and path as a string from the PathBuf
                 */
                let this_file_path = file_path.path().into_os_string().into_string().unwrap();
                let file_name = file_path.path().file_name().unwrap().to_os_string().into_string().unwrap();

                /*
                 * Insert into the database next
                 */
                match nksc_param_paths.get_key_value(&file_name) {
                    Some(file_path) => {
                        let cmd = format!(
                            "INSERT OR IGNORE INTO files (path,created,orig_file_name,nksc_path) VALUES('{}','{}','{}','{}');",
                            this_file_path, created_datetime, file_name, file_path.1
                        );
                        QuickNonReturningSqlCommand(cmd);
                    }
                    _ => {
                        let cmd = format!("INSERT OR IGNORE INTO files (path,created,orig_file_name) VALUES('{}','{}','{}');", this_file_path, created_datetime, file_name);
                        QuickNonReturningSqlCommand(cmd);
                    }
                }
            } else {
                /* Directory, at this stage no plans to add recursion, but this is where we would put it. For now,
                 * we will just use it to potentially parse and/or find the nikon params directory
                 */
            }
        }
    } else {
        println!("Something went gravely wrong: {:?}", WhichDirectory.file_name());
    }
}

/// Chcecks to see if there is a Nikon side car file, and then goes on to insert the details into the main database
fn check_and_add_this_file_to_the_database(file_path: String) {
    let test_Path = PathBuf::from(&file_path);
    if test_Path.is_file() {
        let nksc_path = get_nksc_file_path(&test_Path);
        let created_datetime = get_file_created_timestamp_as_iso8601(&test_Path);
        let orig_file_name = test_Path.file_name().unwrap().to_os_string().into_string().unwrap();

        if !nksc_path.is_empty() {
            let cmd = format!(
                "INSERT OR IGNORE INTO files (path,created,orig_file_name,nksc_path) VALUES('{}','{}','{}','{}');",
                file_path, created_datetime, orig_file_name, nksc_path
            );
            println!("{}", cmd);
            QuickNonReturningSqlCommand(cmd);
        } else {
            let cmd = format!("INSERT OR IGNORE INTO files (path,created,orig_file_name) VALUES('{}','{}','{}');", file_path, created_datetime, orig_file_name);
            println!("{}", cmd);
            QuickNonReturningSqlCommand(cmd);
        }
    }
}

/// Executes a script to delete any unwanted files from the drag and drop and then unsets the protection flag
//
// GetFileSpec returns something like *.nef;*.jpg;*.jpeg, which we have to turn into
// something like: DELETE FROM files WHERE lower(orig_file_name) NOT LIKE lower('%.nef') AND lower(orig_file_name) NOT LIKE lower('%.jpg') AND lower(orig_file_name) NOT LIKE lower('%jpeg')
fn delete_unwanted_files_after_bulk_import() {
    let mut spec: String = String::new();
    GetFileSpec(IDC_PREFS_DRAG_N_DROP.try_into().unwrap(), &mut spec);
    spec = spec.replace(";*", "') AND lower(orig_file_name) NOT LIKE lower('%");
    spec = spec.replace('*', "lower('%");
    spec.push_str("')");

    let cmd = format!(
        r#"DELETE FROM files WHERE tmp_lock=0 AND lower(orig_file_name) NOT LIKE {};
                       UPDATE files SET tmp_lock=0;"#,
        spec
    );
    QuickNonReturningSqlCommand(cmd);
}

/// Gets the file created time stamp from a given file in iso8601 format
fn get_file_created_timestamp_as_iso8601(file_path: &PathBuf) -> String {
    let metadata = fs::metadata(file_path.as_path()).unwrap();
    if let Ok(created_time) = metadata.created() {
        let timestamp: DateTime<Local> = (created_time).into();
        format!("{}", timestamp.format("%+"))
    } else {
        "".to_string()
    }
}

/// Function to find out of there are any user settings for NX Studio
///
/// Returns a PathBuff, which may be empty, so also check to see if it was successful or not
fn find_nx_studio_FileData_db() -> (PathBuf, bool) {
    let mut success = false;
    let mut localappdata = env::var("LOCALAPPDATA").expect("$LOCALAPPDATA is not set.");
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
            let mut localappdata = env::var("LOCALAPPDATA").expect("$LOCALAPPDATA is not set.");
            localappdata.push_str("\\Nikon\\NX Studio\\DB\\FileData.db");

            self.location = PathBuf::from(&localappdata);

            /*
             * See if the file exists, if it does, change success to true
             */
            if self.location.exists() {
                self.success = true;
            } else {
                self.success = false;
            }
        }
        self.success
    }
}

/// Function for saving a resource from the executable. Prints out an error message if not successful.
///
/// Rust's create file will, by default overwrites any existing files, which happens if the reset settings button is pressed.
fn ResourceSave(id: i32, section: &str, filename: &str) {
    unsafe {
        let the_asset: Result<_, _> = FindResourceA(None, PCSTR(id as *mut u8), PCSTR(section.as_ptr()));

        match the_asset {
            Ok(ResourceHandle) => {
                let GlobalMemoryBlock = LoadResource(None, ResourceHandle);
                let ptMem = LockResource(GlobalMemoryBlock.unwrap());
                let dwSize: usize = SizeofResource(None, ResourceHandle).try_into().unwrap();
                let slice = slice::from_raw_parts(ptMem as *const u8, dwSize);

                let mut output = File::create(filename).expect("Create file failed. 😮");
                output.write_all(&slice[0..dwSize]).expect("Write failed. 😥");
                drop(output);
            }
            Err(e) => println!("Error {}", e),
        }
    }
}

/// Our "web service" to handle internal database requests
///
/// The server is a blocking server, so it only accepts a single request at a time.
/// A large part of this is because sqlite, while seemingly okay with concurrent reads, most definately
/// does not like concurrent writes.
//
fn mem_db() {
    let server = Server::http(HOST).unwrap_or_else(|_| panic!("{}{}{}", "Setting up the internal HTTP server (", HOST, ") failed.😫"));
    let mut host: String = String::new();
    let mut bonafide: String = String::new();
    let mut rng = rand::thread_rng();

    /*
     * BONAFIDE is a global variable randomally generated by the server and expected
     * in the header of any requests sent to the server. This is used as a very simple
     * security measure to ensure only internal requests are accepted.
     */
    unsafe {
        BONAFIDE = format!("{}", rng.gen_range(0..65535));
    }

    /*
     * Next we will open up our in-memory sqlite database which will eventually be used for lots of things.
     * After opening it we will attach the settings database to it and copy the settings across.
     */
    if let Ok(db) = Connection::open("c:/dev/in_memory.sqlite") {
        // Used for debugging
        //           if let Ok(db) = Connection::open_in_memory() { // Used for production

        ReloadSettings_(&db);
        // Create the table which will hold all of the file names
        db.execute_batch(
            r#"DROP TABLE IF EXISTS files;
               CREATE TABLE files (
                    idx INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL UNIQUE, 
                    path TEXT NOT NULL, /* Full path to image file */
                    created DATETIME, /* The time file file was created in seconds since Unix epoc */ 
                    orig_file_name TEXT, 
                    new_file_name TEXT,
                    nksc_path TEXT, /* Path to the Nikon sidecar file */
                    inNXstudio BOOL DEFAULT 0, /* has an entry in the NX Studio sqlite database */
                    tmp_lock BOOL DEFAULT 0, /* Temporary lock for internal use */
                    locked BOOL DEFAULT 0, /* Name change manually locked */

                    UNIQUE(path, orig_file_name)
               );
            "#,
        )
        .expect("Setting up the file table failed.");

        /*
         *  Server loop
         */
        for request in server.incoming_requests() {
            println!("received request! method: {:?}, url: {:?}, headers: {:?}", request.method(), request.url(), request.headers());

            /*
             *  Check the headers sent to us to ensure the request has come from our program and not somewhere else.
             *  We check firstly to see if its come from localhost, then make sure it also has sent the secret bonafide key.
             */
            for header in request.headers() {
                if header.field.as_str() == "Host" {
                    host = header.value.to_string();
                } else if header.field.as_str() == "X-Bonafide" {
                    bonafide = header.value.to_string();
                }
            }

            unsafe {
                if bonafide != BONAFIDE || host != HOST {
                    FailU!("A request to mem_db() came from an UNVERIFIED or UNKNOWN souruce.😲\r\rAborting!");
                    EXITERMINATE = true;
                    panic!("mem_db() terminated after receiving a request from an unknown or foriegn source.😤");
                }
            }

            // Extract our command from the http request
            let command = decode(request.url().trim_start_matches('/')).unwrap();
            let mut response = Response::from_string("Not cool");

            /*
             *  Run our loop to process commands
             *  These ideally should be kind of sorted from largest command string to smallest just
             *  in case there is some overlap in the beginning of the strings.
             */
            if command.starts_with("GetIntSetting") {
                let cmd = format!("SELECT value FROM settings where ID={}", command.get(14..).expect("Extracting ID failed."));
                let mut stmt = db.prepare(&cmd).unwrap();
                let answer = stmt.query_row([], |row| row.get(0) as Result<u32>).expect("No results?");
                response = Response::from_string(format!("{}", answer));
                //
            } else if command.starts_with("SetIntSetting") {
                let value_delimeter = command.rfind('=').unwrap();
                let value = command.get(value_delimeter + 1..).unwrap();
                let id = command.get(14..value_delimeter).unwrap();
                let cmd = format!("UPDATE settings SET value={} WHERE id={};", value, id);
                db.execute(&cmd, []).expect("SetIntSetting() failed.");
                response = Response::from_string("Okay");
                //
            } else if command.starts_with("GetTextSetting") {
                let cmd = format!("SELECT value FROM settings where ID={}", command.get(14..).expect("Extracting ID failed."));
                let mut stmt = db.prepare(&cmd).unwrap();
                let answer = stmt.query_row([], |row| row.get(0) as Result<String>).expect("No results?");
                response = Response::from_string(answer.to_string());
                //
            } else if command.starts_with("SaveSettings") {
                SaveSettings_(&db);
                response = Response::from_string("Okay");
                //
            } else if command.starts_with("ReloadSettings") {
                ReloadSettings_(&db);
                response = Response::from_string("Okay");
                //
            } else if command.starts_with("Count") {
                let table_delimeter = command.rfind('=').unwrap();
                let table = command.get(table_delimeter + 1..).unwrap();
                let what = command.get(6..table_delimeter).unwrap();
                let cmd = format!("SELECT COUNT( DISTINCT {}) FROM {};", what, table);
                let mut stmt = db.prepare(&cmd).unwrap();
                let answer = stmt.query_row([], |row| row.get(0) as Result<u32>).expect("No results?");
                response = Response::from_string(format!("{}", answer));
                //
            } else if command.starts_with("GetFilePatterns") {
                let idx = command.get(16..).unwrap();
                let cmd = format!("SELECT pszName, pszSpec FROM file_pat WHERE idx={};", idx);
                let mut stmt = db.prepare(&cmd).unwrap();
                let pszName = stmt.query_row([], |row| row.get(0) as Result<String>).expect("No results?");
                let pszSpec = stmt.query_row([], |row| row.get(1) as Result<String>).expect("No results?");
                response = Response::from_string(format!("{}&{}", pszName, pszSpec));
                //
            } else if command.starts_with("DeleteFilePattern") {
                let pszName = command.get(18..).unwrap();
                let cmd = format!("DELETE FROM file_pat WHERE pszName='{}';", pszName);
                db.execute(&cmd, []).expect("DeleteFilePattern() failed.");
                response = Response::from_string("Okay");
                //
            } else if command.starts_with("MakeTempFilePatternDatabase") {
                let cmd = "DROP TABLE IF EXISTS tmp_file_pat; CREATE TABLE tmp_file_pat AS SELECT * FROM file_pat;".to_string();
                db.execute_batch(&cmd).expect("MakeTempFilePatternDatabase() failed.");
                response = Response::from_string("Okay");
                //
            } else if command.starts_with("RestoreFilePatternDatabase") {
                let cmd = r#"DROP TABLE IF EXISTS file_pat;
                                      CREATE TABLE 'file_pat' 
                                      (
                                          idx INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL UNIQUE, 
                                          pszName TEXT,
                                          pszSpec TEXT
                                      );
                                      INSERT INTO file_pat (pszName, pszSpec) SELECT pszName, pszSpec FROM tmp_file_pat;
                                      DROP TABLE IF EXISTS tmp_file_pat"#
                    .to_string();
                db.execute_batch(&cmd).expect("RestoreFilePatternDatabase() failed.");
                response = Response::from_string("Okay");
                //
            } else if command.starts_with("AddFilePattern") {
                let idx_delimeter = command.find('=').unwrap();
                let zName_delimeter = command.rfind("|+|").unwrap();
                let zSpec_delimeter = command.rfind("|$|").unwrap();
                let idx = command.get(idx_delimeter + 1..zName_delimeter).unwrap();
                let zName = command.get(zName_delimeter + 3..zSpec_delimeter - 1).unwrap();
                let zSpec = command.get(zSpec_delimeter + 3..command.len() - 1).unwrap();

                let cmd = format!(
                    r#"
                DROP TABLE IF EXISTS add_file_pat;
                CREATE TABLE add_file_pat 
                (
                    idx INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL UNIQUE, 
                    pszName TEXT,
                    pszSpec TEXT
                );
                INSERT INTO add_file_pat (pszName, pszSpec) SELECT pszName, pszSpec FROM file_pat WHERE idx <={idx};
                INSERT INTO add_file_pat (pszName, pszSpec) VALUES ('{zName}', '{zSpec}');
                INSERT INTO add_file_pat (pszName, pszSpec) SELECT pszName, pszSpec FROM file_pat WHERE idx >{idx};
                DROP TABLE IF EXISTS file_pat;
                ALTER TABLE add_file_pat RENAME TO file_pat;
                "#,
                    idx = idx,
                    zName = zName,
                    zSpec = zSpec
                );
                db.execute_batch(&cmd).expect("AddFilePattern() failed.");
                response = Response::from_string("Okay");
                //
            } else if command.starts_with("QuickNonReturningSqlCommand") {
                let cmd = command.get(28..command.len() - 1).unwrap();
                db.execute_batch(cmd).expect("QuickNonReturningSqlCommand() failed.");
                response = Response::from_string("Okay");
                //
            } else if command.starts_with("GetFileSpec") {
                let idx = command.get(12..).unwrap();
                let cmd = format!(
                    r#"
                                            SELECT pszSpec FROM file_pat 
                                              WHERE
                                               idx=(SELECT idx FROM file_pat,settings 
                                                        WHERE 
                                                          file_pat.idx=(settings.value + 1) 
                                                          AND id={} 
                                                          AND file_pat.idx
                                                        );               
                                        "#,
                    idx
                );

                let mut stmt = db.prepare(&cmd).unwrap();
                let pszSpec = stmt.query_row([], |row| row.get(0) as Result<String>).expect("No results?");
                response = Response::from_string(pszSpec.to_string());
                //
            }

            // Generate a new key for the next request
            unsafe {
                BONAFIDE = format!("{}", rng.gen_range(0..65535));
            }
            request.respond(response).unwrap();
        }
    } else {
        Fail!("Could not start internal database service. 😯");
    }
}

/// Shorthand function to make the code a little more readable
//
// Twas a bit of a pain to write because both minreq and tinyhttp have the same "Response" name space and
// it took a while to work out what was going wrong!
fn send_cmd(cmd: &str, error_msg: &str) -> minreq::Response {
    let cmd = format!("{}/{}", HOST_URL.to_owned(), cmd);
    unsafe { minreq::get(cmd).with_header("X-Bonafide", BONAFIDE.as_str()).send().expect(error_msg) }
}

/// Get an integer value from the settings database
fn GetIntSetting(id: i32) -> usize {
    let cmd = format!("GetIntSetting={}", id);
    send_cmd(&cmd, "GetIntSetting() failed").as_str().unwrap().parse::<usize>().unwrap()
}

/// Set an integer value from the settings database
fn SetIntSetting(id: i32, value: isize) {
    let cmd = format!("SetIntSetting={}={}", id, value);
    send_cmd(&cmd, "SetIntSetting() failed");
}

/// Get a TEXT value from the settings database
fn GetTextSetting(id: i32) -> usize {
    let cmd = format!("GetTextSetting={}", id);
    send_cmd(&cmd, "GetTextSetting() failed").as_str().unwrap().parse::<usize>().unwrap()
}

/// Wrapper function to reload settings database from disc
fn ReloadSettings() {
    send_cmd("ReloadSettings", "ReloadSettings() failed");
}

/// Function to reload the settings database from disc
fn ReloadSettings_(db: &Connection) {
    unsafe {
        let cmd = format!(
            r#"DROP TABLE IF EXISTS 'settings';
            CREATE TABLE 'settings' (name,ID,value);
            DROP TABLE IF EXISTS file_pat;
            CREATE TABLE 'file_pat' 
              (
                idx INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL UNIQUE, 
                pszName TEXT,
                pszSpec TEXT
              );
            ATTACH DATABASE '{}' AS SETTINGS;
              INSERT INTO main.settings SELECT * FROM settings.settings;
              INSERT INTO file_pat (pszName, pszSpec) SELECT pszName, pszSpec FROM settings.load_filterspec;
            DETACH DATABASE SETTINGS;"#,
            path_to_settings_sqlite
        );
        db.execute_batch(&cmd).expect("ReloadSettings_() failed.");
    }
}

/// Save the settings to disc
fn SaveSettings() {
    send_cmd("SaveSettings", "SaveSettings() failed");
}

/// Function to save the settings to disc
fn SaveSettings_(db: &Connection) {
    unsafe {
        let cmd = format!(
            r#"ATTACH DATABASE '{}' AS SETTINGS;
            DELETE FROM settings.settings WHERE id IN (SELECT id FROM main.settings);
            INSERT INTO settings.settings SELECT * FROM main.settings;
            DROP TABLE settings.load_filterspec;
            CREATE TABLE settings.load_filterspec (idx INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL UNIQUE, pszName TEXT, pszSpec TEXT);
            INSERT INTO settings.load_filterspec (pszName, pszSpec) SELECT pszName, pszSpec FROM main.file_pat ORDER BY idx;
            DETACH DATABASE SETTINGS"#,
            path_to_settings_sqlite
        );
        db.execute_batch(&cmd).expect("SaveSettings_() failed.");
    }
}

/// Transfer settings from the dialog boxes in the preferences screen to the in memory settings database
fn ApplySettings(hwnd: HWND) {
    unsafe {
        SetIntSetting(IDC_PREFS_ON_CONFLICT, SendDlgItemMessageA(hwnd, IDC_PREFS_ON_CONFLICT, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_ON_CONFLICT_ADD, SendDlgItemMessageA(hwnd, IDC_PREFS_ON_CONFLICT_ADD, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_ON_CONFLICT_NUM, SendDlgItemMessageA(hwnd, IDC_PREFS_ON_CONFLICT_NUM, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_DATE_SHOOT_PRIMARY, SendDlgItemMessageA(hwnd, IDC_PREFS_DATE_SHOOT_PRIMARY, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_DATE_SHOOT_SECONDARY, SendDlgItemMessageA(hwnd, IDC_PREFS_DATE_SHOOT_SECONDARY, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_DRAG_N_DROP, SendDlgItemMessageA(hwnd, IDC_PREFS_DRAG_N_DROP, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_NX_STUDIO, IsDlgButtonChecked(hwnd, IDC_PREFS_NX_STUDIO).try_into().unwrap());
    }
}

/// Counts the number of <what>s in a <table> which resides in our in memory database
fn Count(what: &str, table: &str) -> usize {
    let cmd = format!("Count={}={}", what, table);
    send_cmd(&cmd, "Count() failed").as_str().unwrap().parse::<usize>().unwrap()
}

/// Gets file masks/patterns from our in memory database
fn GetFilePatterns(idx: usize, zName: &mut String, zSpec: &mut String) {
    let cmd = format!("GetFilePatterns={}", idx);
    let answer = send_cmd(&cmd, "GetFilePatterns() failed");
    let answer = answer.as_str().unwrap();
    let delimeter = answer.rfind('&').unwrap();
    *zName = answer.get(..delimeter).unwrap().to_string();
    *zSpec = answer.get(delimeter + 1..).unwrap().to_string();
}

/// Gets file speccs from our in memory database
fn GetFileSpec(idx: usize, zSpec: &mut String) {
    let cmd = format!("GetFileSpec={}", idx);
    let answer = send_cmd(&cmd, "GetFileSpec() failed");
    let answer = answer.as_str().unwrap();
    *zSpec = answer.to_string();
}

/// Deletes a file masks/patterns from our in memory database
fn DeleteFilePattern(zName: &mut String) {
    let cmd = format!("DeleteFilePattern={}", zName);
    send_cmd(&cmd, "DeleteFilePattern() failed");
}

/// Makes a temporary copy of the file pattern table in our in-memory database
fn MakeTempFilePatternDatabase() {
    send_cmd("MakeTempFilePatternDatabase", "MakeTempFilePatternDatabase() failed");
}

/// Restores the default file patterns
fn RestoreFilePatternDatabase() {
    send_cmd("RestoreFilePatternDatabase", "RestoreFilePatternDatabase() failed");
}

/// Gets file masks/patterns from our in memory database
fn AddFilePattern(idx: usize, zName: String, zSpec: String) {
    let cmd = format!("AddFilePattern={}|+|{}|$|{}", idx, zName, zSpec);
    send_cmd(&cmd, "AddFilePattern() failed");
}

/// Runs a non-returning batch sql script
fn QuickNonReturningSqlCommand(sql: String) {
    let cmd = format!("QuickNonReturningSqlCommand={}", sql);
    send_cmd(&cmd, "QuickNonReturningSqlCommand() failed");
}
