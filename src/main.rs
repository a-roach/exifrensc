#![allow(unused_parens)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use core::mem::transmute;
use std::{
    collections::HashMap,
    convert::TryInto,
    env,
    ffi::OsStr,
    fs,
    fs::File,
    io::{BufRead, BufReader, Write},
    mem,
    mem::size_of,
    os::raw::c_void,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    slice,
    sync::{
        mpsc,
        mpsc::{Receiver, Sender},
        Mutex,
    },
    thread,
};
//use std::ptr::null;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{Com::*, LibraryLoader::*, Threading::*},
        UI::{
            Controls::{LIST_VIEW_ITEM_STATE_FLAGS, LVITEMA_GROUP_ID, *},
            Input::KeyboardAndMouse::{EnableWindow, SetFocus},
            Shell::{Common::COMDLG_FILTERSPEC, *},
            WindowsAndMessaging::*,
        },
    },
};
// use windows::Win32::UI::Shell::SHCreateItemInKnownFolder;
// use windows::Win32::{System::Environment::GetCurrentDirectoryA};
use chrono::{prelude::Local, DateTime};
use db::*;
use exif::In;

include!("resource_defs.rs");

#[macro_use]
mod macros;
mod db;

// Global Variables
pub static mut path_to_settings_sqlite: String = String::new();
pub static mut NX_Studio: NxStudioDB = NxStudioDB { location: String::new(), success: false }; // Path to NX Studio
pub static mut MAIN_HWND: HWND = windows::Win32::Foundation::HWND(0);
static mut MAIN_THREAD_ID: u32 = 0; // The thread ID of our main process
static mut thinking: Thinking = Thinking { thread_id: 0, hwnd: HWND(0) }; // Structure which pins our progress bars down
static mut WANT_TO_STOP_FILE_SCANNING: bool = false; // Siginal to threads running lengthy operations with the progress dialog to try and stop
static mut RESULT_SENDER: Option<Mutex<Sender<DBcommand>>> = None; // A channel used by our database server to take requests
pub static mut MAIN_LISTVIEW_RESULTS: Vec<(String, String, usize)> = Vec::new(); // Results passed from our database thread to UI thread

pub const KAMADAK_EXIF: usize = 1;
pub const EXIFTOOL: usize = 0;

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
    unsafe { MAIN_THREAD_ID = GetCurrentThreadId() };

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

    unsafe {
        NX_Studio.existant();
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
            MAIN_HWND = CreateDialogParamA(hinst, PCSTR(IDD_MAIN as *mut u8), HWND(0), Some(main_dlg_proc), LPARAM(0));
            let mut message = MSG::default();

            /*
             * Setup and launch our database server in a separate thread
             * We are not ever going to kill this thread, we will let it run in the background until the program terminates,
             * so there is no reason to grab its thread Id or anything like that.
             */
            let (tx, rx) = mpsc::channel();
            RESULT_SENDER = Some(Mutex::new(tx));

            let _db_thread = thread::spawn(|| {
                mem_db(rx);
            });

            check_settings_version();

            /*
             * Our windows message loop
             */
            while GetMessageA(&mut message, HWND(0), 0, 0).into() {
                if (IsDialogMessageA(MAIN_HWND, &message) == false) {
                    TranslateMessage(&message);
                    DispatchMessageA(&message);
                }
            }
        }
        Ok(())
    }
}

/// Function which has some wordy and repaeted in-line code. Just here to make the code
/// more readable. It will see if the user has selected something, and if so, will return
/// the file path which we use as a unique key in our database.
fn GetSelectedPath() -> String {
    unsafe {
        let dlgFileList: HWND = GetDlgItem(MAIN_HWND, IDC_MAIN_FILE_LIST);
        let n = SendMessageA(dlgFileList, LVM_GETSELECTEDCOUNT, WPARAM(0), LPARAM(0));
        let mut name: String = String::new();
        if n.0 > 0 {
            let selected = SendMessageA(dlgFileList, LVM_GETSELECTIONMARK, WPARAM(0), LPARAM(0));

            let name_buffer = [0; 260_usize];
            let lv = LVITEMW {
                mask: LVIF_TEXT,
                iItem: 0,
                iSubItem: 0,
                state: LIST_VIEW_ITEM_STATE_FLAGS(0),
                stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
                pszText: transmute(name_buffer.as_ptr()),
                cchTextMax: 260,
                iImage: 0,
                lParam: LPARAM(0),
                iIndent: 0,
                iGroupId: LVITEMA_GROUP_ID(0),
                cColumns: 0,
                puColumns: std::ptr::null_mut(),
                piColFmt: std::ptr::null_mut(),
                iGroup: 0,
            };

            SendMessageA(dlgFileList, LVM_GETITEMTEXT, WPARAM(selected.0.try_into().unwrap()), LPARAM(&lv as *const _ as isize));

            let mut utf7_buffer: [u8; 260] = [0; 260_usize];
            let mut i = 0;
            let mut j = 0;

            /*
             * Convert to ASCII/UTF7 (kind of 🙄)
             */
            while name_buffer[i] != 0 {
                utf7_buffer[j] = name_buffer[i];
                i += 2;
                j += 1;
            }
            name = String::from_utf8_unchecked(utf7_buffer.to_vec());
            name.truncate(name.find('\0').unwrap());
        }
        name
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
                segoe_mdl2_assets.set_text(IDC_MAIN_ADD_FOLDER, w!("\u{F89A}"), w!("Add a folder full of photos"));
                segoe_mdl2_assets.set_text(IDC_MAIN_SAVE, w!("\u{E74E}"), w!("Save changes to names"));
                segoe_mdl2_assets.set_text(IDC_MAIN_RENAME, w!("\u{E8AC}"), w!("Manually rename selected photo"));
                segoe_mdl2_assets.set_text(IDC_MAIN_ERASE, w!("\u{ED60}"), w!("Remove selected photo from the list"));
                segoe_mdl2_assets.set_text(IDC_MAIN_DELETE, w!("\u{ED62}"), w!("Remove all photos from the list"));
                segoe_mdl2_assets.set_text(IDC_MAIN_LOCK, w!("\u{E72E}"), w!("Remove all photos from the list"));
                segoe_mdl2_assets.set_text(IDC_MAIN_EXIF, w!("\u{E8EC}"), w!("Remove all photos from the list"));
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
                    WPARAM(
                        (LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER | LVS_EX_DOUBLEBUFFER)
                            .try_into()
                            .unwrap(),
                    ),
                    LPARAM(
                        (LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT | LVS_NOSORTHEADER | LVS_EX_DOUBLEBUFFER)
                            .try_into()
                            .unwrap(),
                    ),
                );

                let mut lvC = LVCOLUMNA {
                    mask: LVCF_FMT | LVCF_TEXT | LVCF_SUBITEM | LVCF_WIDTH,
                    fmt: LVCFMT_LEFT,
                    cx: convert_x_to_client_coords(IDC_MAIN_FILE_LIST_R.width) - convert_x_to_client_coords(IDC_MAIN_FILE_LIST_R.width / 4) - 52,
                    pszText: transmute(w!("Original File Name").as_ptr()),
                    cchTextMax: 0,
                    iSubItem: 0,
                    iImage: 0,
                    iOrder: 0,
                    cxMin: 50,
                    cxDefault: 55,
                    cxIdeal: 55,
                };

                SendDlgItemMessageW(hwnd, IDC_MAIN_FILE_LIST, LVM_INSERTCOLUMN, WPARAM(0), LPARAM(&lvC as *const _ as isize));

                lvC.cx = convert_x_to_client_coords(IDC_MAIN_FILE_LIST_R.width / 4);
                lvC.iSubItem = 1;
                lvC.pszText = transmute(w!("Rename to").as_ptr());
                SendDlgItemMessageW(hwnd, IDC_MAIN_FILE_LIST, LVM_INSERTCOLUMN, WPARAM(1), LPARAM(&lvC as *const _ as isize));

                lvC.iSubItem = 2;
                lvC.cx = 52;
                lvC.pszText = transmute(w!("Locked").as_ptr());
                SendDlgItemMessageW(hwnd, IDC_MAIN_FILE_LIST, LVM_INSERTCOLUMN, WPARAM(2), LPARAM(&lvC as *const _ as isize));

                1
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam); // I am sure there has to be a better way to do this, but the only way I could get the value out of a WPARAM type was to transmute it to a u64
                wParam = (wParam << 48 >> 48); // LOWORD isn't defined, at least as far as I could tell, so I had to improvise

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL {
                    segoe_mdl2_assets.destroy();
                    PostQuitMessage(0);
                } else {
                    match wParam as i32 {
                        IDC_MAIN_ADD_PICTURE | IDM_ADD_PICTURE => {
                            LoadPictureFiles();
                        }
                        IDC_MAIN_ADD_FOLDER | IDM_ADD_FOLDER_OF_PICTURES => {
                            LoadDirectoryOfPictures();
                        }
                        IDC_MAIN_SAVE => {
                            transfer_data_to_main_file_list();
                        }
                        IDC_MAIN_DELETE | IDM_CLEAR_LIST => {
                            let n = SendDlgItemMessageA(MAIN_HWND, IDC_MAIN_FILE_LIST, LVM_GETITEMCOUNT, WPARAM(0), LPARAM(0));
                            if n.0 > 0 && MessageBoxA(None, s!("Are you sure you want to clear the list?"), s!("Clear list"), MB_YESNO | MB_ICONEXCLAMATION) == IDYES {
                                QuickNonReturningSqlCommand("DELETE FROM exif;DELETE FROM files;".to_owned());
                                SendDlgItemMessageA(MAIN_HWND, IDC_MAIN_FILE_LIST, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
                            }
                        }
                        IDC_MAIN_ERASE | IDM_REMOVE_FROM_LIST => {
                            let filepath = GetSelectedPath();
                            if !filepath.is_empty() {
                                DeleteFromDatabase(filepath);
                                let dlgFileList = GetDlgItem(MAIN_HWND, IDC_MAIN_FILE_LIST);
                                let selected = SendMessageA(dlgFileList, LVM_GETSELECTIONMARK, WPARAM(0), LPARAM(0));
                                SendMessageA(dlgFileList, LVM_DELETEITEM, WPARAM(selected.0.try_into().unwrap()), LPARAM(0));
                            }
                        }
                        IDC_MAIN_SYNC => {
                            prerename_files();
                            transfer_data_to_main_file_list();
                        }
                        IDC_MAIN_RENAME | IDM_MANUALLY_RENAME => {
                            let selected = SendMessageA(GetDlgItem(hwnd, IDC_MAIN_FILE_LIST), LVM_GETSELECTIONMARK, WPARAM(0), LPARAM(0));
                            DialogBoxParamA(hinst, PCSTR(IDD_MANUALLY_RENAME as *mut u8), hwnd, Some(manual_rename_dlg_proc), LPARAM(selected.0));
                        }
                        IDM_LOCK | IDC_MAIN_LOCK => {
                            let filepath = GetSelectedPath();
                            if !filepath.is_empty() {
                                let state = ToggleLock(filepath);

                                if 1 == 2 {
                                    let dlgFileList = GetDlgItem(MAIN_HWND, IDC_MAIN_FILE_LIST);
                                    let selected = SendMessageA(dlgFileList, LVM_GETSELECTIONMARK, WPARAM(0), LPARAM(0));
                                    let mut lock_image: String = "🔓".to_owned();

                                    if state == 1 {
                                        lock_image = "🔒".to_owned();
                                    }
                                    lock_image.push('\0');

                                    let lv = LVITEMW {
                                        mask: LVIF_TEXT,
                                        iItem: selected.0.try_into().unwrap(),
                                        iSubItem: 2,
                                        state: LIST_VIEW_ITEM_STATE_FLAGS(0),
                                        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
                                        pszText: transmute(utf8_to_utf16(&lock_image).as_ptr()),
                                        cchTextMax: 0,
                                        iImage: 0,
                                        lParam: LPARAM(0),
                                        iIndent: 0,
                                        iGroupId: LVITEMA_GROUP_ID(0),
                                        cColumns: 0,
                                        puColumns: std::ptr::null_mut(),
                                        piColFmt: std::ptr::null_mut(),
                                        iGroup: 0,
                                    };

                                    SendDlgItemMessageW(MAIN_HWND, IDC_MAIN_FILE_LIST, LVM_SETITEMTEXT, WPARAM(selected.0.try_into().unwrap()), LPARAM(&lv as *const _ as isize));
                                    SendDlgItemMessageW(MAIN_HWND, IDC_MAIN_FILE_LIST, LVM_REDRAWITEMS, WPARAM(selected.0.try_into().unwrap()), LPARAM(selected.0));
                                    UpdateWindow(dlgFileList);
                                } else {
                                    transfer_data_to_main_file_list();
                                }
                            }
                        }
                        IDM_EXIF_BROWSER | IDC_MAIN_EXIF => {
                            let filepath = GetSelectedPath();
                            if !filepath.is_empty() {
                                let exif_hwnd: HWND = CreateDialogParamA(hinst, PCSTR(IDD_EXIF_Browser as *mut u8), HWND(0), Some(exif_browse_dlg_proc), LPARAM(0));
                                transfer_data_to_exif_browser_list(exif_hwnd, &filepath);
                            }
                        }
                        IDC_MAIN_SETTINGS => {
                            DialogBoxParamA(hinst, PCSTR(IDD_SETTINGS as *mut u8), HWND(0), Some(settings_dlg_proc), LPARAM(0));
                        }

                        IDC_MAIN_INFO => {
                            CreateDialogParamA(hinst, PCSTR(IDD_ABOUT as *mut u8), HWND(0), Some(about_dlg_proc), LPARAM(0));
                        }
                        _ => {}
                    }
                }
                1
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
                    new_width - convert_x_to_client_coords(IDC_MAIN_FILE_LIST_R.x + 7),
                    new_height - convert_y_to_client_coords(IDC_MAIN_FILE_LIST_R.y + 7),
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
                1
            }

            // Strangely, WM_DROPFILES does not work when this program is run from an console!🤔
            WM_DROPFILES => {
                let mut file_name_buffer = [0; MAX_PATH as usize];
                let hDrop: HDROP = HDROP(transmute(wParam));
                let nFiles: u32 = DragQueryFileA(hDrop, 0xFFFFFFFF, Some(file_name_buffer.as_mut_slice())); // Wish I could send a NULL as the last param since I don't really need to pass a buffer for this call
                if nFiles > 1 {
                    thinking.launch(nFiles as isize, PCWSTR(utf8_to_utf16("Scanning files\0").as_ptr()));
                } else {
                    thinking.launch(BAR_MARQUEE, PCWSTR(utf8_to_utf16("Scanning files\0").as_ptr()));
                }

                /*
                 * We will just run a "protection" flag over any current files which are in our database
                 * to ensure they do not get deleted in the last step which is removing any files dropped
                 * into the database which are not images.
                 */

                QuickNonReturningSqlCommand("BEGIN;UPDATE files SET tmp_lock=1;COMMIT;BEGIN;".to_string());

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
                        CheckAndAddThisFile(file_path);
                        if WANT_TO_STOP_FILE_SCANNING {
                            break;
                        }
                        thinking.step(1);
                    }
                }

                delete_unwanted_files_after_bulk_import();
                Commit!();
                check_if_in_NXstudio();
                fill_in_missing_DateTimeOriginal();
                transfer_data_to_main_file_list();

                thinking.kill();
                DragFinish(hDrop);
                1
            }

            WM_NOTIFY => {
                if (lParamTOnmhdr(transmute(lParam)).0 == IDC_MAIN_FILE_LIST) && (lParamTOnmhdr(transmute(lParam)).1 == NM_RCLICK) {
                    /*
                     * Setup our right-click context menu
                     */

                    let mut xy = POINT { x: 0, y: 0 };
                    let rootmenu: HMENU = LoadMenuW(hinst, PCWSTR(IDR_MAIN_FILE_LIST as *mut u16)).unwrap();
                    let myPopup: HMENU = GetSubMenu(rootmenu, 0);
                    GetCursorPos(&mut xy);
                    TrackPopupMenu(myPopup, TPM_TOPALIGN | TPM_LEFTALIGN, xy.x, xy.y, 0, hwnd, None);
                }
                1
            }

            WM_DESTROY => {
                PostQuitMessage(0);
                1
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
                segoe_mdl2_assets.set_text(IDC_PREFSAddAMask, w!("\u{F8AA}"), w!("Add new file pattern"));
                segoe_mdl2_assets.set_text(IDC_PREFSDelPattern, w!("\u{E74D}"), w!("Delete file pattern"));
                segoe_mdl2_assets.set_text(IDC_PREFSExifToolBrowse, w!("\u{ED25}"), w!("Set path to ExifTool.exe"));

                SendDlgItemMessageA(hwnd, IDC_PREFS_ExifToolPath, EM_SETLIMITTEXT, WPARAM(MAX_PATH as usize), LPARAM(0));
                let mut ExifToolPath = GetTextSetting(IDC_PREFS_ExifToolPath);
                ExifToolPath.push('\0');
                let ExifToolPath = utf8_to_utf16(&ExifToolPath);
                SetDlgItemTextW(hwnd, IDC_PREFS_ExifToolPath, PCWSTR(ExifToolPath.as_ptr()));

                /*
                 * Set up our combo boxes
                 */
                SendDlgItemMessageW(hwnd, IDC_PREFS_ON_CONFLICT, CB_ADDSTRING, WPARAM(0), LPARAM(w!("Add\0").as_ptr() as isize));
                SendDlgItemMessageW(hwnd, IDC_PREFS_ON_CONFLICT, CB_ADDSTRING, WPARAM(0), LPARAM(w!("Skip\0").as_ptr() as isize));
                SendDlgItemMessageA(hwnd, IDC_PREFS_ON_CONFLICT, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_ON_CONFLICT)), LPARAM(0));

                SendDlgItemMessageW(hwnd, IDC_PREFS_EXIF_Engine, CB_ADDSTRING, WPARAM(0), LPARAM(w!("Phil Harvey's ExifTool\0").as_ptr() as isize));
                SendDlgItemMessageW(hwnd, IDC_PREFS_EXIF_Engine, CB_ADDSTRING, WPARAM(0), LPARAM(w!("Kamadak EXIF\0").as_ptr() as isize));
                SendDlgItemMessageA(hwnd, IDC_PREFS_EXIF_Engine, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_EXIF_Engine)), LPARAM(0));
                segoe_mdl2_assets.set_text(
                    IDC_PREFS_EXIF_Engine,
                    w!(""),
                    w!("ExifTool requires an external program, which you have to install, but decodes tags more throughly and you will get many private tags (that may not be useful, but are interesting).\r\rKamadak is internal, not as comprehensive, but probably gives you everything you need.\r\rEach represent some tag values in slightly different ways."),
                );

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
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_NUM, CB_ADDSTRING, WPARAM(0), LPARAM(w!("01\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_ON_CONFLICT_NUM, CB_ADDSTRING, WPARAM(0), LPARAM(w!("001\0").as_ptr() as isize));
                SendMessageA(dlgIDC_PREFS_ON_CONFLICT_NUM, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_ON_CONFLICT_NUM)), LPARAM(0));

                let dlgIDC_PREFS_DATE_SHOOT_PRIMARY: HWND = GetDlgItem(hwnd, IDC_PREFS_DATE_SHOOT_PRIMARY);
                SendMessageW(dlgIDC_PREFS_DATE_SHOOT_PRIMARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("DateTimeOriginal in the EXIF data\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_DATE_SHOOT_PRIMARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("the \"File Created\" date\0").as_ptr() as isize));
                SendMessageW(dlgIDC_PREFS_DATE_SHOOT_PRIMARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("the \"Last Modified\" date\0").as_ptr() as isize));
                SendMessageA(dlgIDC_PREFS_DATE_SHOOT_PRIMARY, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_DATE_SHOOT_PRIMARY)), LPARAM(0));

                SendDlgItemMessageW(hwnd, IDC_PREFS_DATE_SHOOT_SECONDARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("use \"File Created\" date\0").as_ptr() as isize));
                SendDlgItemMessageW(hwnd, IDC_PREFS_DATE_SHOOT_SECONDARY, CB_ADDSTRING, WPARAM(0), LPARAM(w!("use \"Last Modified\" date\0").as_ptr() as isize));
                SendDlgItemMessageA(hwnd, IDC_PREFS_DATE_SHOOT_SECONDARY, CB_SETCURSEL, WPARAM(GetIntSetting(IDC_PREFS_DATE_SHOOT_SECONDARY)), LPARAM(0));

                /*
                 * Setup up the file mask box, which is a listview
                 * Kind of in parallel we will also set up the drag and drop filter box at the same time
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
                        puColumns: std::ptr::null_mut(),
                        piColFmt: std::ptr::null_mut(),
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

                let NX_stu_DlgItem: HWND = GetDlgItem(hwnd, IDC_PREFS_NX_STUDIO);

                if !NX_Studio.existant() {
                    EnableWindow(NX_stu_DlgItem, false);
                    SendMessageA(NX_stu_DlgItem, BM_SETCHECK, WPARAM(BST_UNCHECKED.0.try_into().unwrap()), LPARAM(0));
                } else if GetIntSetting(IDC_PREFS_NX_STUDIO) == 1 {
                    SendMessageA(NX_stu_DlgItem, BM_SETCHECK, WPARAM(BST_CHECKED.0.try_into().unwrap()), LPARAM(0));
                } else {
                    SendMessageA(NX_stu_DlgItem, BM_SETCHECK, WPARAM(BST_UNCHECKED.0.try_into().unwrap()), LPARAM(0));
                }
                1
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
                            let name_buffer = [0; 128_usize];
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
                        DialogBoxParamA(hinst, PCSTR(IDD_ADD_FILE_MASK as *mut u8), hwnd, Some(add_file_mask_dlg_proc), LPARAM(selected.0));
                    }
                    IDC_PREFSExifToolBrowse => {
                        let file_dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).unwrap();

                        // Change a few of the default options for the dialog
                        file_dialog.SetTitle(w!("Path to ExifTool.exe")).expect("SetTitle() failed");
                        file_dialog.SetOkButtonLabel(w!("Set")).expect("SetOkButtonLabel() failed");
                        let file_pat: [COMDLG_FILTERSPEC; 1] = [COMDLG_FILTERSPEC {
                            pszName: w!("ExifTool"),
                            pszSpec: w!("ExifTool.exe"),
                        }];
                        file_dialog.SetFileTypes(&file_pat).unwrap();

                        let answer = file_dialog.Show(None); // Basically an error means no file was selected

                        if let Ok(__dummy) = answer {
                            let selected_file = file_dialog.GetResult().unwrap(); // IShellItem with the result. We know we have a result because we have got this far.
                            let file_name = selected_file.GetDisplayName(SIGDN_FILESYSPATH).unwrap(); // Pointer to a utf16 buffer with the file name
                            let ExifToolPath = utf8_to_utf16(&file_name.to_string().unwrap());
                            SetDlgItemTextW(hwnd, IDC_PREFS_ExifToolPath, PCWSTR(ExifToolPath.as_ptr()));
                            SetTextSetting(IDC_PREFS_ExifToolPath, file_name.to_string().unwrap());
                            CoTaskMemFree(Some(transmute(file_name.0)));
                        }
                    }
                    _ => {}
                }
                1
            }
            /*             WM_CONTEXTMENU =>{
                           println!("WM_CONTEXTMENU");
                           1
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
                1
            }

            WM_DESTROY => {
                EndDialog(hwnd, 0);
                1
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

                1
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
                        puColumns: std::ptr::null_mut(),
                        piColFmt: std::ptr::null_mut(),
                        iGroup: 0,
                    };

                    SendMessageW(dlgFileMask, LVM_INSERTITEM, WPARAM(0), LPARAM(&lv as *const _ as isize));
                    lv.pszText = transmute(utf8_to_utf16(&fileMask).as_ptr());
                    lv.iSubItem = 1;
                    SendMessageW(dlgFileMask, LVM_SETITEMTEXT, WPARAM(selected_.0.try_into().unwrap()), LPARAM(&lv as *const _ as isize));

                    AddFilePattern(selected_.0.try_into().unwrap(), patDescription, fileMask);

                    EndDialog(hwnd, 0);
                }
                1
            }

            WM_DESTROY => {
                EndDialog(hwnd, 0);
                0
            }
            _ => 0,
        }
    }
}

/// Dialog callback for our manual rename dialog
//
extern "system" fn manual_rename_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    static mut selected_: LPARAM = LPARAM(0);
    unsafe {
        match nMsg {
            WM_INITDIALOG => {
                set_icon(hwnd);
                SendDlgItemMessageA(hwnd, IDC_MANUALLY_RENAME_Text, EM_SETLIMITTEXT, WPARAM(64), LPARAM(0));
                SetFocus(GetDlgItem(hwnd, IDC_MANUALLY_RENAME_Text));
                selected_ = lParam;
                let path = GetSelectedPath();
                let mut new_file_name = Get_new_file_name(path);
                new_file_name.push('\0');
                SetDlgItemTextW(hwnd, IDC_MANUALLY_RENAME_Text, PCWSTR(utf8_to_utf16(&new_file_name).as_ptr()));
                1
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam);
                wParam = (wParam << 48 >> 48); // LOWORD

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL {
                    EndDialog(hwnd, 0);
                    //
                } else if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDOK {
                    let mut text: [u16; 128] = [0; 128];
                    let len = GetWindowTextW(GetDlgItem(hwnd, IDC_MANUALLY_RENAME_Text), &mut text);
                    let mut new_file_name = String::from_utf16_lossy(&text[..len as usize]);

                    let path = GetSelectedPath();
                    let cmd = format!("UPDATE files SET new_file_name='{new_file_name}' WHERE path='{path}';");
                    QuickNonReturningSqlCommand(cmd);

                    new_file_name.push('\0');

                    let lv = LVITEMW {
                        mask: LVIF_TEXT,
                        iItem: selected_.0.try_into().unwrap(),
                        iSubItem: 1,
                        state: LIST_VIEW_ITEM_STATE_FLAGS(0),
                        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
                        pszText: transmute(utf8_to_utf16(&new_file_name).as_ptr()),
                        cchTextMax: 0,
                        iImage: 0,
                        lParam: LPARAM(0),
                        iIndent: 0,
                        iGroupId: LVITEMA_GROUP_ID(0),
                        cColumns: 0,
                        puColumns: std::ptr::null_mut(),
                        piColFmt: std::ptr::null_mut(),
                        iGroup: 0,
                    };

                    SendDlgItemMessageW(MAIN_HWND, IDC_MAIN_FILE_LIST, LVM_SETITEMTEXT, WPARAM(selected_.0.try_into().unwrap()), LPARAM(&lv as *const _ as isize));
                    EndDialog(hwnd, 0);
                }
                1
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
/// We get our build data from resources_def.rs, which is regenerated each time we do a build.
extern "system" fn about_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, _lParam: LPARAM) -> isize {
    // Have to be global because we need to destroy our font resources eventually
    static mut segoe_bold_9: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) };
    static mut segoe_bold_italic_13: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) };
    static mut segoe_italic_10: WindowsControlText = WindowsControlText { hwnd: HWND(0), hfont: HFONT(0) };

    unsafe {
        match nMsg {
            WM_INITDIALOG => {
                set_icon(hwnd);

                segoe_bold_9.register_font(hwnd, s!("Segoe UI"), 9, FW_BOLD.0, false);
                segoe_bold_9.set_font(IDC_ABOUT_ST_VER);
                segoe_bold_9.set_font(IDC_ABOUT_BUILT);
                segoe_bold_9.set_font(IDC_ABOUT_ST_AUTHOR);
                segoe_bold_9.set_font(IDC_ABOUT_ST_COPY);

                segoe_bold_italic_13.register_font(hwnd, s!("Segoe UI"), 13, FW_BOLD.0, true);
                segoe_bold_italic_13.set_font(IDC_ABOUT_TITLE);

                segoe_italic_10.register_font(hwnd, s!("Segoe UI"), 10, FW_NORMAL.0, true);
                segoe_italic_10.set_font(IDC_ABOUT_DESCRIPTION);

                SetDlgItemTextA(hwnd, IDC_ABOUT_VERSION, PCSTR(PROGRAM_VERSION.as_ptr()));
                SetDlgItemTextA(hwnd, IDC_ABOUT_BUILDDATE, PCSTR(ISO_8601_BUILD_STAMP.as_ptr()));
                SetDlgItemTextA(hwnd, IDC_COPYRIGHT, PCSTR(PROGRAM_COPYRIGHT.as_ptr()));

                1
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
                1
            }

            WM_DESTROY => {
                segoe_bold_9.destroy();
                segoe_bold_italic_13.destroy();
                segoe_italic_10.destroy();
                EndDialog(hwnd, 0);
                1
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

struct Thinking {
    thread_id: u32,
    hwnd: HWND,
}

pub const BAR_MARQUEE: isize = 0;

/// Progress bar functions to show we are doing things
impl Thinking {
    /// Launches a progress bar.
    /// If nCount = 0, or BAR_MARQUEE, then the bar is launched as a marquee bar with an indeterminate range.
    /// If nCount >0, then the bar is launched as a range bar with the maximum range set to nCount.
    /// if caption is null(), then the caption will default to "Thinking"
    // CONTROL         "", IDC_PROGRESS, PROGRESS_CLASS, PBS_MARQUEE, 8, 14, 171, 11, WS_EX_LEFT
    #[allow(dead_code)]
    fn launch(&mut self, nCount: isize, caption: PCWSTR) {
        if self.thread_id == 0 {
            let (thread_id_tx, thread_id_rx) = mpsc::channel();
            let (hwnd_tx, hwnd_rx) = mpsc::channel();
            let range: isize = nCount << 16;

            let _thinking_thread = thread::spawn(move || unsafe {
                let hinst = GetModuleHandleA(None).unwrap();
                thread_id_tx.send(GetCurrentThreadId()).unwrap();
                let hwnd = CreateDialogParamA(hinst, PCSTR(IDD_THINKING as *mut u8), HWND(0), Some(thinking_dlg_proc), LPARAM(range));
                hwnd_tx.send(hwnd).unwrap();
                let mut message = MSG::default();
                while GetMessageA(&mut message, HWND(0), 0, 0).into() {
                    if (IsDialogMessageA(hwnd, &message) == false) {
                        TranslateMessage(&message);
                        DispatchMessageA(&message);
                    }
                }
            });

            unsafe {
                WANT_TO_STOP_FILE_SCANNING = false;
            }

            self.thread_id = thread_id_rx.recv().unwrap();
            self.hwnd = hwnd_rx.recv().unwrap();
            if !caption.is_null() {
                unsafe {
                    SetWindowTextW(self.hwnd, caption);
                }
            }
        }
    }

    /// Kills the progress bar.
    #[allow(dead_code)]
    fn kill(&mut self) {
        unsafe {
            PostThreadMessageA(self.thread_id, WM_QUIT, WPARAM(1), LPARAM(0));
            self.thread_id = 0;
            self.hwnd = HWND(0);
        }
    }

    /// Changes our progress bar to a marquee progress bar.
    #[allow(dead_code)]
    fn make_marquee(&mut self) {
        unsafe {
            let mut current_style: isize = GetWindowLongPtrA(GetDlgItem(self.hwnd, IDC_PROGRESS), GWL_STYLE);
            current_style |= PBS_MARQUEE as isize;

            SendDlgItemMessageA(self.hwnd, IDC_PROGRESS, PBM_SETMARQUEE, WPARAM(1), LPARAM(0));
            SetWindowLongPtrA(GetDlgItem(self.hwnd, IDC_PROGRESS), GWL_STYLE, (current_style));
            BringWindowToTop(self.hwnd);
        }
    }

    /// Changes our progress bar to a range progress bar.
    /// nCount = max range
    #[allow(dead_code)]
    fn make_range(&mut self, nCount: isize) {
        unsafe {
            let mut current_style: isize = GetWindowLongPtrA(GetDlgItem(self.hwnd, IDC_PROGRESS), GWL_STYLE);
            current_style ^= PBS_MARQUEE as isize;
            let range: isize = nCount << 16;
            SetWindowLongPtrA(GetDlgItem(self.hwnd, IDC_PROGRESS), GWL_STYLE, (current_style));
            SendDlgItemMessageA(self.hwnd, IDC_PROGRESS, PBM_SETSTEP, WPARAM(1), LPARAM(0));
            SendDlgItemMessageA(self.hwnd, IDC_PROGRESS, PBM_SETRANGE, WPARAM(0), LPARAM(range));
            BringWindowToTop(self.hwnd);
        }
    }

    /// Increments our progress bar
    /// n = the number to increase it by.
    // If n > 0, then we use getpos/setpos to move the progress bar, otherwise we just use step.
    #[allow(dead_code)]
    fn step(&mut self, n: isize) {
        unsafe {
            if n != 1 {
                let current_position = SendDlgItemMessageA(self.hwnd, IDC_PROGRESS, PBM_GETPOS, WPARAM(0), LPARAM(0));
                let new_position = current_position.0 + n;
                SendDlgItemMessageA(self.hwnd, IDC_PROGRESS, PBM_SETPOS, WPARAM(new_position.try_into().unwrap()), LPARAM(0));
            } else {
                SendDlgItemMessageA(self.hwnd, IDC_PROGRESS, PBM_STEPIT, WPARAM(0), LPARAM(0));
            }
            BringWindowToTop(self.hwnd);
        }
    }

    /// Changes our progress bar's title.
    #[allow(dead_code)]
    fn set_caption(&mut self, caption: PCWSTR) {
        unsafe {
            SetWindowTextW(self.hwnd, caption);
            BringWindowToTop(self.hwnd);
        }
    }
}

/// Callback function for processing the thinking/progress bar
extern "system" fn thinking_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    unsafe {
        match nMsg {
            WM_INITDIALOG => {
                /*
                 * Choose, and set up for either a range bar or a marquee
                 */
                if lParam == LPARAM(0) {
                    SendDlgItemMessageA(hwnd, IDC_PROGRESS, PBM_SETMARQUEE, WPARAM(1), LPARAM(0));
                } else {
                    let mut current_style: isize = GetWindowLongPtrA(GetDlgItem(hwnd, IDC_PROGRESS), GWL_STYLE);
                    current_style ^= PBS_MARQUEE as isize;
                    SetWindowLongPtrA(GetDlgItem(hwnd, IDC_PROGRESS), GWL_STYLE, (current_style));
                    SendDlgItemMessageA(hwnd, IDC_PROGRESS, PBM_SETRANGE, WPARAM(0), lParam);
                    SendDlgItemMessageA(hwnd, IDC_PROGRESS, PBM_SETSTEP, WPARAM(1), LPARAM(0));
                }
                1
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam);
                wParam = (wParam << 48 >> 48); // LOWORD

                if MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDCANCEL || MESSAGEBOX_RESULT(wParam.try_into().unwrap()) == IDOK {
                    EndDialog(hwnd, 0);
                } else if wParam == IDC_THINKING_Cancel as u64 {
                    WANT_TO_STOP_FILE_SCANNING = true;
                }
                1
            }

            WM_DESTROY => {
                EndDialog(hwnd, 0);
                1
            }

            WM_ACTIVATEAPP => {
                BringWindowToTop(hwnd);
                1
            }
            _ => 0,
        }
    }
}

/// Dialog callback function for our EXIF browsing window
extern "system" fn exif_browse_dlg_proc(hwnd: HWND, nMsg: u32, wParam: WPARAM, lParam: LPARAM) -> isize {
    unsafe {
        match nMsg {
            WM_INITDIALOG => {
                set_icon(hwnd);

                /*
                 * Setup up our listview
                 */

                SendDlgItemMessageW(
                    hwnd,
                    IDC_EXIF_BROWSER_List,
                    LVM_SETEXTENDEDLISTVIEWSTYLE,
                    WPARAM((LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_NOSORTHEADER | LVS_EX_DOUBLEBUFFER).try_into().unwrap()),
                    LPARAM((LVS_EX_TWOCLICKACTIVATE | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP | LVS_NOSORTHEADER | LVS_EX_DOUBLEBUFFER).try_into().unwrap()),
                );

                let mut lvC = LVCOLUMNA {
                    mask: LVCF_FMT | LVCF_TEXT | LVCF_SUBITEM | LVCF_WIDTH,
                    fmt: LVCFMT_LEFT,
                    cx: convert_x_to_client_coords(IDC_EXIF_BROWSER_List_R.width) - convert_x_to_client_coords(IDC_EXIF_BROWSER_List_R.width / 2) - 17,
                    pszText: transmute(w!("EXIF Tag").as_ptr()),
                    cchTextMax: 0,
                    iSubItem: 0,
                    iImage: 0,
                    iOrder: 0,
                    cxMin: 50,
                    cxDefault: 55,
                    cxIdeal: 55,
                };

                SendDlgItemMessageW(hwnd, IDC_EXIF_BROWSER_List, LVM_INSERTCOLUMN, WPARAM(0), LPARAM(&lvC as *const _ as isize));

                lvC.cx = convert_x_to_client_coords(IDC_EXIF_BROWSER_List_R.width / 2) - 3;
                lvC.iSubItem = 1;
                lvC.pszText = transmute(w!("Value").as_ptr());
                SendDlgItemMessageW(hwnd, IDC_EXIF_BROWSER_List, LVM_INSERTCOLUMN, WPARAM(1), LPARAM(&lvC as *const _ as isize));

                1
            }

            WM_COMMAND => {
                let mut wParam: u64 = transmute(wParam); // I am sure there has to be a better way to do this, but the only way I could get the value out of a WPARAM type was to transmute it to a u64
                wParam = (wParam << 48 >> 48); // LOWORD isn't defined, at least as far as I could tell, so I had to improvise

                if wParam as i32 == IDC_EXIF_Browse_Cancel || wParam as i32 == ID_CANCEL {
                    EndDialog(hwnd, 0);
                }
                1
            }

            WM_SIZE => {
                let mut new_width: u64 = transmute(lParam);
                new_width = (new_width << 48 >> 48); // LOWORD
                let new_width: i32 = new_width.try_into().unwrap();
                let mut new_height: u64 = transmute(lParam);
                new_height = (new_height << 32 >> 48); // HIWORD
                let new_height: i32 = new_height.try_into().unwrap();

                SetWindowPos(
                    GetDlgItem(hwnd, IDC_EXIF_BROWSER_List_R.id) as HWND,
                    HWND_TOP,
                    convert_x_to_client_coords(IDC_EXIF_BROWSER_List_R.x),
                    convert_y_to_client_coords(IDC_EXIF_BROWSER_List_R.y),
                    new_width - convert_x_to_client_coords(IDC_EXIF_BROWSER_List_R.x + 7),
                    new_height - convert_y_to_client_coords(IDC_EXIF_BROWSER_List_R.y + 26),
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                SetWindowPos(
                    GetDlgItem(hwnd, IDC_EXIF_Browse_Cancel_R.id) as HWND,
                    HWND_TOP,
                    new_width - convert_x_to_client_coords(57),
                    new_height - convert_y_to_client_coords(21),
                    convert_x_to_client_coords(IDC_EXIF_Browse_Cancel_R.width),
                    convert_y_to_client_coords(IDC_EXIF_Browse_Cancel_R.height),
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                1
            }

            WM_DESTROY => {
                EndDialog(hwnd, 0);
                1
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

    /// Set the caption and tool tip text of a windows control.
    /// If we set the caption, the font of the control is also set. If you only want to set the font, use the setfont option.
    /// If we only set the tooltip, not fonts are changed. It is just a short cut for setting a tooltip.
    fn set_text(&self, id: i32, caption: PCWSTR, tooltip_text: PCWSTR) {
        unsafe {
            let hinst = GetModuleHandleA(None).unwrap();

            if caption != w!("") {
                SendDlgItemMessageA(self.hwnd, id, WM_SETFONT, WPARAM(self.hfont.0 as usize), LPARAM(0));
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

    /// Set the font of a windows control.
    fn set_font(&self, id: i32) {
        unsafe {
            SendDlgItemMessageA(self.hwnd, id, WM_SETFONT, WPARAM(self.hfont.0 as usize), LPARAM(0));
        }
    }

    /// Delete the font resource when we are done with it.
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

/// Opens up a dialog so a user can select multiple image files and inser into our database.
//fn LoadFile() -> Result<()> {
fn LoadPictureFiles() {
    unsafe {
        let file_dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).unwrap();

        // Change a few of the default options for the dialog
        file_dialog.SetTitle(w!("Choose Photos to Rename")).expect("SetTitle() failed in LoadPictureFiles()");
        file_dialog.SetOkButtonLabel(w!("Select Photos")).expect("SetOkButtonLabel() failed in LoadPictureFiles()");

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

        let defPath: IShellItem = SHCreateItemInKnownFolder(&FOLDERID_Pictures, 0, None).expect("Could not find Pictures");
        file_dialog.SetFolder(&defPath).unwrap(); // SetDefaultFolder
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
                CheckAndAddThisFile(file_name.to_string().unwrap());

                CoTaskMemFree(Some(transmute(file_name.0))); // feel rather nervy about this - not sure this is trying to free the right thing
            }
            check_if_in_NXstudio();
            fill_in_missing_DateTimeOriginal();
            transfer_data_to_main_file_list();
        }

        //file_dialog.Release();
    }
    //    Ok(())
}

/// Opens a dialog and lets users pick a folder of pictures to rename.
/// Automatically inserts into our database.    
//fn LoadDirectory() -> Result<()> {
fn LoadDirectoryOfPictures() {
    unsafe {
        let file_dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).unwrap();
        file_dialog.SetTitle(w!("Choose Directories of Photos to Add")).expect("SetTitle() failed in LoadDirectory()");
        file_dialog.SetOkButtonLabel(w!("Select Directories")).expect("SetOkButtonLabel() failed in LoadDirectory()");
        let defPath: IShellItem = SHCreateItemInKnownFolder(&FOLDERID_Pictures, 0, None).expect("Could not find Pictures");
        file_dialog.SetFolder(&defPath).unwrap(); // SetDefaultFolder
        let mut options = file_dialog.GetOptions().unwrap();
        options.0 = options.0 | FOS_PICKFOLDERS.0 | FOS_ALLOWMULTISELECT.0;
        file_dialog.SetOptions(options).expect("SetOptions() failed in LoadDirectory()");

        let answer = file_dialog.Show(None); // Basically an error means no file was selected
        if let Ok(_v) = answer {
            let selected_directories = file_dialog.GetResult().unwrap(); // IShellItem with the result. We know we have a result because we have got this far.
            let directory_name = selected_directories.GetDisplayName(SIGDN_FILESYSPATH).unwrap(); // Pointer to a utf16 buffer with the file name
            QuickNonReturningSqlCommand("BEGIN;UPDATE files SET tmp_lock=1;COMMIT;BEGIN;".to_string());
            WalkDirectoryAndAddFiles(&PathBuf::from(directory_name.to_string().unwrap()));
            delete_unwanted_files_after_bulk_import();
            Commit!();
            check_if_in_NXstudio();
            fill_in_missing_DateTimeOriginal();
            transfer_data_to_main_file_list();
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
/// Function makes three passes: the first time looking for the Nikon params directory, from which it will grab a copy internally
/// so it can map out where the corrosponding entry is; then it looks for the files; finally it fetches the exif tags for the files
fn WalkDirectoryAndAddFiles(WhichDirectory: &PathBuf) {
    unsafe {
        thinking.launch(BAR_MARQUEE, PCWSTR(utf8_to_utf16("Scanning files\0").as_ptr()));
        if WhichDirectory.is_dir()
        // Sanity check, probably not necessary, but this is Rust and Rust is all about "safety"
        {
            let nksc_param_path = WhichDirectory.clone().join("NKSC_PARAM");
            let mut nksc_param_paths = HashMap::new();
            let mut nksc_path = String::new();
            let mut nksc_name = String::new();
            let (stdout_transmitter, rx) = mpsc::channel();
            let stderr_transmitter = stdout_transmitter.clone();
            const sizeof_ChildStdin: usize = size_of::<std::process::ChildStdin>();
            let tmpbuf: [u8; sizeof_ChildStdin] = [0; sizeof_ChildStdin]; // ChildStdin is private internally so for now we'll reserve a block of memory for it 🙄
            let mut exiftool_stdin: std::process::ChildStdin = transmute(tmpbuf.as_ptr());
            let engine = GetIntSetting(IDC_PREFS_EXIF_Engine);

            /*
             * If we are using ExifTool, we will run the ExifTool and keep it open in the backgound for now
             */
            if engine == EXIFTOOL {
                let ExifToolPath = GetTextSetting(IDC_PREFS_ExifToolPath);

                /*
                 * Create a new process and spawn ExifTool into that process
                 *  • We are going to run ExifTool in "stay open" mode and send commands to it from stdin.
                 *    Because of this, we need to steal stdin for input, stdout to capture the output, and
                 *    stderr so we can monitor for errors. Parsing stderr and stdout will ultimately happen
                 *    in parallel threads so we don't lock up anything.
                 */
                let mut exiftool_process = Command::new(ExifToolPath)
                    .args(["-stay_open", "true", "-@", "-"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();

                // Take ownership of stdout so we can pass to a separate thread.
                let exiftool_stdout = exiftool_process.stdout.take().expect("Could not take stdout");

                // Take ownership of stdin so we can pass to a separate thread.
                let exiftool_stderr = exiftool_process.stderr.take().expect("Could not take stderr");

                // Grab stdin so we can pipe commands to ExifTool
                exiftool_stdin = exiftool_process.stdin.unwrap();

                // Create a separate thread to loop over stdout
                let _stdout_thread = thread::spawn(move || {
                    let stdout_lines = BufReader::new(exiftool_stdout).lines();

                    for line in stdout_lines {
                        let line = line.unwrap();

                        // Check to see if our processing has finished, if it has we will send a message to our main thread.
                        if line == "{ready}" {
                            stdout_transmitter.send(line).unwrap();
                        } else {
                            /*
                             * Example returns from our command:
                             *[File] - FileModifyDate: 2022:11:01 01:39:18+10:00
                             *[EXIF] 36867 DateTimeOriginal: 2022:10:31 15:37:25
                             *[Composite] - SubSecCreateDate: 2022:10:31 15:37:25.0180+10:00
                             *[XMP]->[XMP] - DateTimeDigitized: 2022:12:25 12:06:41+10:00
                             *[IPTC]->[IPTC] 80 By-line: Someone
                             *
                             * We are not interested in the File types, binary data, or marker notes so we will not process them.
                             */

                            if !line.contains("use -b option to extract)") {
                                let exif_type_delimeter = line.find(' ').unwrap();
                                let exif_type = line.get(..exif_type_delimeter).unwrap();
                                if exif_type == "[EXIF]" || exif_type == "[IPTC]" {
                                    if let Some(exif_tag_delimeter) = line.get(7..).unwrap().find(' ') {
                                        if let Some(exif_id) = line.get(7..7 + exif_tag_delimeter) {
                                            if let Some(exif_value_delimeter) = line.find(':') {
                                                if let Some(exif_tag) = line.get(7 + exif_tag_delimeter + 1..exif_value_delimeter) {
                                                    if let Some(exif_value) = line.get(exif_value_delimeter + 2..) {
                                                        if !exif_value.is_empty() {
                                                            let cmd = format!("INSERT OR IGNORE INTO exif (path,tag,tag_id,value) VALUES('file_path','{}',{},'{}');", exif_tag, exif_id, exif_value.to_string().replace('\"', ""));
                                                            QuickNonReturningSqlCommand(cmd);
                                                        }
                                                    } else {
                                                        Warning!("Extracting exif_value failed.");
                                                    }
                                                } else {
                                                    Warning!("Extracting failed.");
                                                }
                                            } else {
                                                Warning!("Finding exif_value_delimeter failed looking for a :");
                                            }
                                        } else {
                                            Warning!("exif_id failed");
                                        }
                                    } else {
                                        Warning!("exif_tag_delimeter failed");
                                    }
                                } else if exif_type == "[Composite]" || exif_type == "[XMP]" {
                                    if let Some(exif_value_delimeter) = line.find(':') {
                                        if let Some(exif_tag) = line.get(exif_type_delimeter + 3..exif_value_delimeter) {
                                            if let Some(exif_value) = line.get(exif_value_delimeter + 2..) {
                                                if !exif_value.is_empty() {
                                                    let cmd = format!("INSERT OR IGNORE INTO exif (path,tag,value) VALUES('file_path','{}','{}');", exif_tag, exif_value.to_string().replace('\"', ""));
                                                    QuickNonReturningSqlCommand(cmd);
                                                }
                                            } else {
                                                Warning!("Extracting exif_value failed.");
                                            }
                                        } else {
                                            Warning!("Extracting exif_tag failed.");
                                        }
                                    } else {
                                        Warning!("Finding exif_value_delimeter failed looking for a :");
                                    }
                                }
                            }
                        }
                    }
                });

                /*
                 * Create a separate thread to loop over stderr
                 * Anything which comes through stderr will just be sent back to our calling thread, and will trip an error.
                 */
                let _stderr_thread = thread::spawn(move || {
                    let stderr_lines = BufReader::new(exiftool_stderr).lines();
                    for line in stderr_lines {
                        let line = line.unwrap();
                        stderr_transmitter.send(line).unwrap();
                    }
                });
            }

            /*
             * Look for the sidecar directory, nksc, then populate our HashMap with the key,
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
            let paths = fs::read_dir(WhichDirectory).expect("Could not count the files in the directory 😥.");
            let file_count = paths.count();
            thinking.make_range(file_count as isize);
            let paths = fs::read_dir(WhichDirectory).expect("Could not scan the directory 😥.");

            for each_path in paths {
                let file_path = each_path.unwrap();

                if (file_path.path().is_file()) {
                    let created_mod_datetime = get_file_created_modified_timestamp_as_iso8601(&file_path.path());

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
                                "INSERT OR IGNORE INTO files (path,created,modified,orig_file_name,nksc_path) VALUES('{}','{}','{}','{}','{}');",
                                this_file_path, created_mod_datetime.0, created_mod_datetime.1, file_name, file_path.1
                            );
                            QuickNonReturningSqlCommand(cmd);
                        }
                        _ => {
                            let cmd = format!(
                                "INSERT OR IGNORE INTO files (path,created,modified,orig_file_name) VALUES('{}','{}','{}','{}');",
                                this_file_path, created_mod_datetime.0, created_mod_datetime.1, file_name
                            );
                            QuickNonReturningSqlCommand(cmd);
                        }
                    }

                    /*
                     * Next we will read the Exif data and insert into our database
                     */
                    if engine == KAMADAK_EXIF {
                        let file = std::fs::File::open(file_path.path()).unwrap();
                        let mut bufreader = std::io::BufReader::new(&file);
                        let exifreader = exif::Reader::new();

                        if let Ok(exif) = exifreader.read_from_container(&mut bufreader) {
                            for f in exif.fields() {
                                if f.ifd_num == In::PRIMARY && f.tag.description().is_some() && f.tag.to_string() != "MakerNote" && f.display_value().to_string() != "0x00000000000000000000000000" {
                                    let cmd = format!(
                                        "INSERT OR IGNORE INTO exif (path,tag,tag_id,value) VALUES('{}','{}',{},'{}');",
                                        file_path.path().as_os_str().to_string_lossy().clone(),
                                        f.tag,
                                        f.tag.number(),
                                        f.display_value().to_string().replace('\"', "").trim()
                                    );
                                    QuickNonReturningSqlCommand(cmd);
                                }
                            }
                        }
                    } else {
                        /*
                         * Send a command through to ExifTool using its stdin pipe, then wait for a response from the thread.
                         * We have to send as "bytes" rather than rust's default UTF16.
                         */
                        let exif_cmd = format!("-G\n-D\n-s2\n-f\n-n\n{}\n-execute\n", file_path.path().as_os_str().to_string_lossy());
                        exiftool_stdin.write_all(exif_cmd.as_bytes()).expect("Failed to pipe a command to ExifTool.😥");
                        let received = rx.recv().unwrap(); // wait for the command to finish
                        if received == "{ready}" {
                            let cmd = format!("UPDATE exif SET path='{}' WHERE path='file_path';", file_path.path().as_os_str().to_string_lossy());
                            QuickNonReturningSqlCommand(cmd);
                        } else {
                            let warn = &format!("Well, that,\"{}\", was not expected!🤔\0", received);
                            MessageBoxW(None, PCWSTR(&utf8_to_utf16(warn) as *const Vec<u16> as *const u16), w!("Warning!"), MB_OK | MB_ICONINFORMATION);
                        }
                    }
                    thinking.step(1);
                    if WANT_TO_STOP_FILE_SCANNING {
                        break;
                    }
                } else {
                    /* Directory, at this stage no plans to add recursion, but this is where we would put it. For now,
                     * we will just use it to potentially parse and/or find the nikon params directory
                     */
                }
            }
            /*
             * Shutdown Exiftool
             */
            if engine == EXIFTOOL {
                exiftool_stdin.write_all(b"-stay_open\nfalse\n-execute\n").expect("Failed to pipe a command to ExifTool.😥");
            }
        } else {
            let mut warn = format!("Something went gravely wrong: {:?}", WhichDirectory.file_name());
            MessageBoxA(None, PCSTR(warn.as_mut_ptr()), s!("Warning!"), MB_OK | MB_ICONINFORMATION);
        }

        thinking.kill();
    }
}

/// Checks to see if there is a Nikon side car file, and then goes on to insert the details into the main database
fn CheckAndAddThisFile(file_path: String) {
    let test_Path = PathBuf::from(&file_path);
    if test_Path.is_file() {
        let nksc_path = get_nksc_file_path(&test_Path);
        let created_mod_datetime = get_file_created_modified_timestamp_as_iso8601(&test_Path);
        let orig_file_name = test_Path.file_name().unwrap().to_os_string().into_string().unwrap();

        /*
         * Pick our exif engine
         * Each have their own pros and cons. The Kamadak EXIF engine is compiled within the program,
         * is quite quick, but does not decode as many tags as ExifTool, and also sometimes get tags
         * a little wrong. Exiftool is quite bullet proof when it comes to decoding, but is noticable
         * slower, but you do get many, many more tags (none of which you may want or need for simple
         * renaming tasks).
         */
        if GetIntSetting(IDC_PREFS_EXIF_Engine) == KAMADAK_EXIF {
            let file = std::fs::File::open(file_path.clone()).unwrap();
            let mut bufreader = std::io::BufReader::new(&file);
            let exifreader = exif::Reader::new();

            if let Ok(exif) = exifreader.read_from_container(&mut bufreader) {
                for f in exif.fields() {
                    if f.ifd_num == In::PRIMARY && f.tag.description().is_some() && f.tag.to_string() != "MakerNote" && f.display_value().to_string() != "0x00000000000000000000000000" {
                        let cmd = format!(
                            "INSERT OR IGNORE INTO exif (path,tag,tag_id,value) VALUES('{}','{}',{},'{}');",
                            file_path,
                            f.tag,
                            f.tag.number(),
                            f.display_value().to_string().replace('\"', "").trim()
                        );
                        QuickNonReturningSqlCommand(cmd);
                    }
                }
            }
        } else {
            // Phil Harvey's ExifTool
            let ExifToolPath = GetTextSetting(IDC_PREFS_ExifToolPath);
            let file_path_copy = file_path.clone();
            /*
             * Set up a channel to let our threads talk to each other
             * We will copy the transmitter so both stderr and stdout have transmitters,
             * but we will have only one receiver in our main thread.
             */
            let (stdout_transmitter, rx) = mpsc::channel();
            let stderr_transmitter = stdout_transmitter.clone();

            // Create a new process and spawn ExifTool into that process
            let mut exiftool_process = Command::new(ExifToolPath)
                .args(["-stay_open", "true", "-@", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();

            // Take ownership of stdout so we can pass to a separate thread.
            let exiftool_stdout = exiftool_process.stdout.take().expect("Could not take stdout");

            // Take ownership of stdin so we can pass to a separate thread.
            let exiftool_stderr = exiftool_process.stderr.take().expect("Could not take stderr");

            // Grab stdin so we can pipe commands to ExifTool
            let exiftool_stdin = exiftool_process.stdin.as_mut().unwrap();

            // Create a separate thread to loop over stdout
            let _stdout_thread = thread::spawn(move || {
                let stdout_lines = BufReader::new(exiftool_stdout).lines();

                for line in stdout_lines {
                    let line = line.unwrap();

                    // Check to see if our processing has finished, if it has we will send a message to our main thread.
                    if line == "{ready}" {
                        stdout_transmitter.send(line).unwrap();
                    } else if !line.contains("use -b option to extract)") {
                        let exif_type_delimeter = line.find(' ').unwrap();
                        let exif_type = line.get(..exif_type_delimeter).unwrap();
                        if exif_type == "[EXIF]" || exif_type == "[IPTC]" {
                            if let Some(exif_tag_delimeter) = line.get(7..).unwrap().find(' ') {
                                if let Some(exif_id) = line.get(7..7 + exif_tag_delimeter) {
                                    if let Some(exif_value_delimeter) = line.find(':') {
                                        if let Some(exif_tag) = line.get(7 + exif_tag_delimeter + 1..exif_value_delimeter) {
                                            if let Some(exif_value) = line.get(exif_value_delimeter + 2..) {
                                                if !exif_value.is_empty() {
                                                    let cmd = format!(
                                                        "INSERT OR IGNORE INTO exif (path,tag,tag_id,value) VALUES('{}','{}',{},'{}');",
                                                        file_path_copy,
                                                        exif_tag,
                                                        exif_id,
                                                        exif_value.to_string().replace('\"', "")
                                                    );
                                                    QuickNonReturningSqlCommand(cmd);
                                                }
                                            } else {
                                                sWarning!("Extracting exif_value failed.");
                                            }
                                        } else {
                                            sWarning!("Extracting failed.");
                                        }
                                    } else {
                                        sWarning!("Finding exif_value_delimeter failed looking for a :");
                                    }
                                } else {
                                    sWarning!("exif_id failed");
                                }
                            } else {
                                sWarning!("exif_tag_delimeter failed");
                            }
                        } else if exif_type == "[Composite]" || exif_type == "[XMP]" {
                            if let Some(exif_value_delimeter) = line.find(':') {
                                if let Some(exif_tag) = line.get(exif_type_delimeter + 3..exif_value_delimeter) {
                                    if let Some(exif_value) = line.get(exif_value_delimeter + 2..) {
                                        if !exif_value.is_empty() {
                                            let cmd = format!("INSERT OR IGNORE INTO exif (path,tag,value) VALUES('{}','{}','{}');", file_path_copy, exif_tag, exif_value.to_string().replace('\"', ""));
                                            QuickNonReturningSqlCommand(cmd);
                                        }
                                    } else {
                                        sWarning!("Extracting exif_value failed.");
                                    }
                                } else {
                                    sWarning!("Extracting exif_tag failed.");
                                }
                            } else {
                                sWarning!("Finding exif_value_delimeter failed looking for a :");
                            }
                        }
                    }
                }
            });

            /*
             * Create a separate thread to loop over stderr
             * Anything which comes through stderr will just be sent back to our calling thread, and will trip an error.
             */
            let _stderr_thread = thread::spawn(move || {
                let stderr_lines = BufReader::new(exiftool_stderr).lines();
                for line in stderr_lines {
                    let line = line.unwrap();
                    stderr_transmitter.send(line).unwrap();
                }
            });

            /*
             * Send a command through to ExifTool using its stdin pipe, then wait for a response from the thread.
             * If successful we should get "{ready}", in which case we could send our next command if we wanted to.
             * We have to send as "bytes" rather than rust's default UTF16.
             */
            let exif_cmd = format!("-G\n-D\n-s2\n-f\n-n\n{}\n-execute\n", file_path);
            exiftool_stdin.write_all(exif_cmd.as_bytes()).expect("Failed to pipe a command to ExifTool.😥");
            let received = rx.recv().unwrap(); // wait for the command to finish
            exiftool_stdin.write_all(b"-stay_open\nfalse\n-execute\n").expect("Failed to pipe a command to ExifTool.😥");
            if received != "{ready}" {
                let mut warn = format!("Well, that,\"{}\", was not expected!🤔\0", received);
                unsafe {
                    MessageBoxA(None, PCSTR(warn.as_mut_ptr()), s!("Warning!"), MB_OK | MB_ICONINFORMATION);
                }
            }
        }

        if !nksc_path.is_empty() {
            let cmd = format!(
                "INSERT OR IGNORE INTO files (path,created,modified,orig_file_name,nksc_path) VALUES('{}','{}','{}','{}','{}');",
                file_path, created_mod_datetime.0, created_mod_datetime.1, orig_file_name, nksc_path
            );
            QuickNonReturningSqlCommand(cmd);
        } else {
            let cmd = format!(
                "INSERT OR IGNORE INTO files (path,created,modified,orig_file_name) VALUES('{}','{}','{}','{}');",
                file_path, created_mod_datetime.0, created_mod_datetime.1, orig_file_name
            );
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

/// Gets the file created  and modified time stamps from a given file in iso8601 format
fn get_file_created_modified_timestamp_as_iso8601(file_path: &PathBuf) -> (String, String) {
    let mut created: String = String::new();
    let mut modified: String = String::new();

    let metadata = fs::metadata(file_path.as_path()).unwrap();
    if let Ok(created_time) = metadata.created() {
        let timestamp: DateTime<Local> = (created_time).into();
        created = format!("{}", timestamp.format("%Y-%m-%d %H:%M:%S%:z")); // was %+
    } else {
        created = "".to_string();
    }

    if let Ok(modified_time) = metadata.modified() {
        let timestamp: DateTime<Local> = (modified_time).into();
        modified = format!("{}", timestamp.format("%Y-%m-%d %H:%M:%S%:z"));
    } else {
        modified = "".to_string();
    }
    (created, modified)
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

pub struct NxStudioDB {
    location: String,
    success: bool,
}

/// Functions pertaining to NX Studio's FileData.db
impl NxStudioDB {
    /// Check to see if FileData.db exists, if it does, set its location and return true, if it doesn't return false
    fn existant(&mut self) -> (bool) {
        if self.location.is_empty() {
            let mut localappdata = env::var("LOCALAPPDATA").expect("$LOCALAPPDATA is not set.");
            localappdata.push_str("\\Nikon\\NX Studio\\DB\\FileData.db");

            let test_path = PathBuf::from(&localappdata);

            /*
             * See if the file exists, if it does, change success to true
             */
            if test_path.exists() {
                self.success = true;
                self.location = test_path.to_string_lossy().to_string();
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
        let the_asset: Result<_> = FindResourceA(None, PCSTR(id as *mut u8), PCSTR(section.as_ptr()));

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

/// Checks to see if there is an entry in the Nx Studion database, and if there is,
/// and the user wants integration with Nx Studio, then adds the Nx Studio file_id for the file.
// So nice NX Studio uses sqlite too!
fn check_if_in_NXstudio() {
    unsafe {
        if GetIntSetting(IDC_PREFS_NX_STUDIO) == 1 {
            let cmd = format!(
                r#"
        ATTACH DATABASE '{}' AS nxstudio;

        UPDATE files set inNXstudio = (
            SELECT
              file_id
            FROM
              nxstudio.file,
              nxstudio.folder
            WHERE
              file.parent_id = folder.folder_id AND
              nxstudio.folder.path||nxstudio.file.name=files.path
          )
        
         WHERE files.path in  (
            SELECT
              path||name as full_path
            FROM
              nxstudio.file,
              nxstudio.folder
            WHERE
              file.parent_id = folder.folder_id
         );"#,
                NX_Studio.location
            );

            Begin!();
            QuickNonReturningSqlCommand(cmd);
            Commit!();
            QuickNonReturningSqlCommand("DETACH DATABASE nxstudio;".to_string());
        }
    }
}

/// Create a synthetic DateTimeOriginal exif tag for files which are missing exif data.
fn fill_in_missing_DateTimeOriginal() {
    let mut cmd: String = String::new();
    if GetIntSetting(IDC_PREFS_DATE_SHOOT_SECONDARY) == 0 {
        cmd = r#"
            INSERT INTO exif (path,tag,tag_id,value)
            SELECT DISTINCT
              files.path, 
              'DateTimeOriginal',
              36867,
              created
            FROM
              files,
              exif
            WHERE
              files.path=exif.path AND
              files.path NOT IN (
                SELECT path
                FROM 
                  exif
                WHERE 
                  tag='DateTimeOriginal'
              );"#
        .to_owned();
    } else {
        cmd = r#"
            INSERT INTO exif (path,tag,tag_id,value)
            SELECT DISTINCT
              files.path, 
              'DateTimeOriginal',
              36867,
              modified
            FROM
              files,
              exif
            WHERE
              files.path=exif.path AND
              files.path NOT IN (
                SELECT path
                FROM 
                  exif
                WHERE 
                  tag='DateTimeOriginal'
              );"#
        .to_owned();
    }
    Begin!();
    QuickNonReturningSqlCommand(cmd);
    Commit!();
}

/// Transfers data from our database to our listview
fn transfer_data_to_main_file_list() {
    unsafe {
        send_cmd("transfer_data_to_main_file_list");

        SendDlgItemMessageA(MAIN_HWND, IDC_MAIN_FILE_LIST, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));

        for (i, item) in MAIN_LISTVIEW_RESULTS.iter().enumerate() {
            let file_path = utf8_to_utf16(&item.0);
            let file_rename = utf8_to_utf16(&item.1);

            let mut lv = LVITEMA {
                mask: LVIF_TEXT,
                iItem: 8192,
                iSubItem: 0,
                state: LIST_VIEW_ITEM_STATE_FLAGS(0),
                stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
                pszText: transmute(file_path.as_ptr()),
                cchTextMax: 0,
                iImage: 0,
                lParam: LPARAM(0),
                iIndent: 0,
                iGroupId: LVITEMA_GROUP_ID(0),
                cColumns: 0,
                puColumns: std::ptr::null_mut(),
                piColFmt: std::ptr::null_mut(),
                iGroup: 0,
            };

            SendDlgItemMessageA(MAIN_HWND, IDC_MAIN_FILE_LIST, LVM_INSERTITEM, WPARAM(0), LPARAM(&lv as *const _ as isize));
            lv.pszText = transmute(file_rename.as_ptr());
            lv.iSubItem = 1;
            SendDlgItemMessageA(MAIN_HWND, IDC_MAIN_FILE_LIST, LVM_SETITEMTEXT, WPARAM(i), LPARAM(&lv as *const _ as isize));
            if item.2 == 0 {
                lv.pszText = transmute(w!("🔓").as_ptr());
            } else {
                lv.pszText = transmute(w!("🔒").as_ptr());
            }
            lv.iSubItem = 2;
            SendDlgItemMessageW(MAIN_HWND, IDC_MAIN_FILE_LIST, LVM_SETITEMTEXT, WPARAM(i), LPARAM(&lv as *const _ as isize));
        }
        MAIN_LISTVIEW_RESULTS.clear();
    }
}

/// Transfers data from our database to our exif tag browser listview
fn transfer_data_to_exif_browser_list(hwnd: HWND, filename: &str) {
    unsafe {
        send_cmd(&format!("transfer_data_to_exif_browser_list{filename}"));

        SendDlgItemMessageA(hwnd, IDC_EXIF_BROWSER_List, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));

        for (i, item) in MAIN_LISTVIEW_RESULTS.iter().enumerate() {
            let exif_tag = utf8_to_utf16(&item.0);
            let exif_value = utf8_to_utf16(&item.1);

            let mut lv = LVITEMA {
                mask: LVIF_TEXT,
                iItem: 8192,
                iSubItem: 0,
                state: LIST_VIEW_ITEM_STATE_FLAGS(0),
                stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
                pszText: transmute(exif_tag.as_ptr()),
                cchTextMax: 0,
                iImage: 0,
                lParam: LPARAM(0),
                iIndent: 0,
                iGroupId: LVITEMA_GROUP_ID(0),
                cColumns: 0,
                puColumns: std::ptr::null_mut(),
                piColFmt: std::ptr::null_mut(),
                iGroup: 0,
            };

            SendDlgItemMessageA(hwnd, IDC_EXIF_BROWSER_List, LVM_INSERTITEM, WPARAM(0), LPARAM(&lv as *const _ as isize));
            lv.pszText = transmute(exif_value.as_ptr());
            lv.iSubItem = 1;
            SendDlgItemMessageA(hwnd, IDC_EXIF_BROWSER_List, LVM_SETITEMTEXT, WPARAM(i), LPARAM(&lv as *const _ as isize));
        }
        MAIN_LISTVIEW_RESULTS.clear();
    }
}

/// Does some sql magic to crate new file names
///
/// First, if any strftime strings are found, use sqlite's internal converter to change DateTimeOriginal into the requested format.
/// Next, if any tags are between $(), it will get said tag value from the exif data for that file. If the tag isn't associated
/// with that file, nothing will be inserted. Within reason, the user can ask for multiple tags to be added.
/// Finally the file name will be cleabed of any illegal characters.
fn prerename_files() {
    unsafe {
        let mut text: [u16; 512] = [0; 512];
        let len = GetWindowTextW(GetDlgItem(MAIN_HWND, IDC_MAIN_PATTERN), &mut text);
        let mut pattern = String::from_utf16_lossy(&text[..len as usize]);
        let mut cmd: String = String::new();

        if pattern.contains('%') {
            cmd = format!(
                r#"
            UPDATE files
            SET new_file_name = new_name
          
            FROM
          
          (  
            SELECT
              CASE
                WHEN locked = 0 THEN
                  STRFTIME('{pattern}', REPLACE(substr(value,0,11),':','-')||' '||SUBSTR(value,12))||
                  '.'||
                  REPLACE(files.path, RTRIM(files.path, REPLACE(files.path, '.', '')), '')
                ELSE
                  IFNULL(new_file_name,orig_file_name)
              END new_name,
              exif.path path
          
            FROM 
              exif,
              files
          
            WHERE 
              tag='DateTimeOriginal' AND
              exif.path = files.path
            ) xx  
          
          WHERE files.path = xx.path;            
            "#
            );
        } else if pattern.contains("$(") {
            cmd = format!(
                r#"
            UPDATE files
              SET new_file_name = new_name
                FROM
                 (  
                  SELECT
                    CASE
                      WHEN locked = 0 THEN
                        '{pattern}'
                      ELSE
                        IFNULL(new_file_name,orig_file_name)
                      END new_name, path
                    FROM files
                  ) xx

            WHERE files.path=xx.path;
                    "#
            );
        }

        while pattern.contains("$(") {
            let start_delimeter = pattern.find("$(").unwrap();
            let end_delimeter = pattern.find(')').unwrap();
            let tag: String = pattern.get(start_delimeter + 2..end_delimeter).unwrap().to_owned();
            let del: String = pattern.get(start_delimeter..end_delimeter + 1).unwrap().to_owned();

            pattern = pattern.replace(&del, "");
            cmd.push_str(&format!(
                r#"
            UPDATE files
            SET new_file_name = new_name
            
            FROM
                (  
                    SELECT
                    CASE
                        WHEN locked = 0 THEN
                        REPLACE(files.new_file_name,'$({tag})',value)
                        ELSE
                        IFNULL(new_file_name,orig_file_name)
                    END new_name,
                    exif.path path
                
                    FROM 
                    exif,
                    files
                
                    WHERE 
                    tag='{tag}' AND
                    exif.path = files.path
                ) xx  
          
            WHERE files.path = xx.path;

            UPDATE files SET new_file_name=REPLACE(new_file_name,'$({tag})','');
               "#
            ));
        }

        if !cmd.is_empty() {
            cmd.push_str(r#"UPDATE files SET new_file_name=REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(new_file_name,':',''),'/',''),'\',''),'*',''),'?',''),'|',''),'"',''),'<',''),'>','');"#);
            QuickNonReturningSqlCommand(cmd);
        }
    }
}
