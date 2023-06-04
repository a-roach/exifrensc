#![allow(unused_parens)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use super::*;
use rusqlite::{Connection, Result};

// Structure to hold our command and also a sender to siginal when the result has come back
pub struct DBcommand {
    tx: mpsc::Sender<String>,
    cmd: String,
}

macro_rules! Fail {
    ($a:expr) => {
        unsafe {
            MessageBoxA(None, s!($a), s!("Error!"), MB_OK | MB_ICONERROR);
        }
    };
}

/// Our "database service" to handle internal database requests
///
/// The server is a blocking server, so it only accepts a single request at a time.
/// A large part of this is because sqlite, while seemingly okay with concurrent reads, most definately
/// does not like concurrent writes.
//
pub fn mem_db(rx: Receiver<DBcommand>) {
    /*
     * We will open up our in-memory sqlite database which will eventually be used for lots of things.
     * After opening it we will attach the settings database to it and copy the settings across.
     */
    if let Ok(db) = Connection::open("R:/in_memory.sqlite") {
        // Used for debugging
        //           if let Ok(db) = Connection::open_in_memory() { // Used for production

        ReloadSettings_(&db);
        // Create the table which will hold all of the file names
        db.execute_batch(
            r#"
               DROP TABLE IF EXISTS files;
               CREATE TABLE files (
                    path TEXT NOT NULL UNIQUE, /* Full path to image file */
                    created DATETIME, /* The time file file was created in seconds since Unix epoc */ 
                    modified DATETIME, /* The time file file was modified in seconds since Unix epoc */ 
                    orig_file_name TEXT, 
                    new_file_name TEXT,
                    nksc_path TEXT, /* Path to the Nikon sidecar file */
                    inNXstudio BOOL DEFAULT 0, /* has an entry in the NX Studio sqlite database */
                    tmp_lock BOOL DEFAULT 0, /* Temporary lock for internal use */
                    locked BOOL DEFAULT 0 /* Name change manually locked */
                );

                DROP TABLE IF EXISTS exif;
                CREATE TABLE exif (
                    path TEXT NOT NULL, /* Full path to the original image file */
                    tag TEXT NOT NULL, /* An exif TAG shorhand in text, as opposed to ID */
                    tag_id,
                    value TEXT NOT NULL, /* The value of the exif tag */
                
                    UNIQUE(path,tag)
                );
            "#,
        )
        .expect("Setting up the file table failed.");

        /*
         *  Server loop
         */
        loop {
            let asked = rx.recv().unwrap(); // This will wait infinitely for a command
            let mut my_response: String = "".to_string();
            //println!("{}",command.cmd);

            /*
             *  Run our loop to process commands
             *  These ideally should be kind of sorted from largest command string to smallest just
             *  in case there is some overlap in the beginning of the strings.
             */
            if asked.cmd.starts_with("GetIntSetting") {
                let cmd = format!("SELECT value FROM settings where ID={}", asked.cmd.get(14..).expect("Extracting ID failed."));
                let mut stmt = db.prepare(&cmd).unwrap();
                let answer = stmt.query_row([], |row| row.get(0) as Result<u32>).expect("No results?");
                my_response = format!("{}", answer);
                //
            } else if asked.cmd.starts_with("SetIntSetting") {
                let value_delimeter = asked.cmd.rfind('=').unwrap();
                let value = asked.cmd.get(value_delimeter + 1..).unwrap();
                let id = asked.cmd.get(14..value_delimeter).unwrap();
                let cmd = format!("UPDATE settings SET value={} WHERE id={};", value, id);
                db.execute(&cmd, []).expect("SetIntSetting() failed.");
                //
            } else if asked.cmd.starts_with("GetTextSetting") {
                let cmd = format!("SELECT value FROM settings where ID={}", asked.cmd.get(14..).expect("Extracting ID failed."));
                let mut stmt = db.prepare(&cmd).unwrap();
                my_response = stmt.query_row([], |row| row.get(0) as Result<String>).expect("No results?");
                //
            } else if asked.cmd.starts_with("SetTextSetting") {
                let value_delimeter = asked.cmd.rfind('=').unwrap();
                let value = asked.cmd.get(value_delimeter + 1..).unwrap();
                let id = asked.cmd.get(15..value_delimeter).unwrap();
                let cmd = format!("UPDATE settings SET value='{}' WHERE id={};", value, id);
                db.execute(&cmd, []).expect("SetTextSetting() failed.");
                //
            } else if asked.cmd.starts_with("SaveSettings") {
                SaveSettings_(&db);
                //
            } else if asked.cmd.starts_with("ReloadSettings") {
                ReloadSettings_(&db);
                //
            } else if asked.cmd.starts_with("Count") {
                let table_delimeter = asked.cmd.rfind('=').unwrap();
                let table = asked.cmd.get(table_delimeter + 1..).unwrap();
                let what = asked.cmd.get(6..table_delimeter).unwrap();
                let cmd = format!("SELECT COUNT( DISTINCT {}) FROM {};", what, table);
                let mut stmt = db.prepare(&cmd).unwrap();
                let answer = stmt.query_row([], |row| row.get(0) as Result<u32>).expect("No results?");
                my_response = format!("{}", answer);
                //
            } else if asked.cmd.starts_with("GetFilePatterns") {
                let idx = asked.cmd.get(16..).unwrap();
                let cmd = format!("SELECT pszName, pszSpec FROM file_pat WHERE idx={};", idx);
                let mut stmt = db.prepare(&cmd).unwrap();
                let pszName = stmt.query_row([], |row| row.get(0) as Result<String>).expect("No results?");
                let pszSpec = stmt.query_row([], |row| row.get(1) as Result<String>).expect("No results?");
                my_response = format!("{}&{}", pszName, pszSpec);
                //
            } else if asked.cmd.starts_with("DeleteFilePattern") {
                let pszName = asked.cmd.get(18..).unwrap();
                let cmd = format!("DELETE FROM file_pat WHERE pszName='{}';", pszName);
                db.execute(&cmd, []).expect("DeleteFilePattern() failed.");
                //
            } else if asked.cmd.starts_with("MakeTempFilePatternDatabase") {
                let cmd = "DROP TABLE IF EXISTS tmp_file_pat; CREATE TABLE tmp_file_pat AS SELECT * FROM file_pat;".to_string();
                db.execute_batch(&cmd).expect("MakeTempFilePatternDatabase() failed.");
                //
            } else if asked.cmd.starts_with("RestoreFilePatternDatabase") {
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
                //
            } else if asked.cmd.starts_with("AddFilePattern") {
                let idx_delimeter = asked.cmd.find('=').unwrap();
                let zName_delimeter = asked.cmd.rfind("|+|").unwrap();
                let zSpec_delimeter = asked.cmd.rfind("|$|").unwrap();
                let idx = asked.cmd.get(idx_delimeter + 1..zName_delimeter).unwrap();
                let zName = asked.cmd.get(zName_delimeter + 3..zSpec_delimeter - 1).unwrap();
                let zSpec = asked.cmd.get(zSpec_delimeter + 3..asked.cmd.len() - 1).unwrap();

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
                //
            } else if asked.cmd.starts_with("QuickNonReturningSqlCommand") {
                let cmd = asked.cmd.get(28..asked.cmd.len() - 1).unwrap();
                db.execute_batch(cmd).expect("QuickNonReturningSqlCommand() failed.");
                //
            } else if asked.cmd.starts_with("GetFileSpec") {
                let idx = asked.cmd.get(12..).unwrap();
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
                my_response = pszSpec.to_string();
                //
            } else if asked.cmd.starts_with("Begin") {
                db.execute("BEGIN;", []).expect("Begin() failed.");
                //
            } else if asked.cmd.starts_with("Commit") {
                db.execute("COMMIT;", []).expect("Commit() failed.");
                //
            } else if asked.cmd.starts_with("transfer_data_to_main_file_list") {
                let mut stmt = db
                    .prepare(
                        r#"
                SELECT 
                  path,
                  ifnull(orig_file_name,new_file_name) new_file_name,
                  locked
                FROM
                  files;                     "#,
                    )
                    .expect("Prepare statement on transfer_data_to_main_file_list failed.");

                unsafe {
                    let mut rows = stmt.query([]);
                    loop {
                        let mut row = rows.as_mut().expect("row in rows failed").next();
                        if row.as_mut().unwrap().is_none() {
                            break;
                        }
                        let mut file_path: String = row.as_mut().unwrap().unwrap().get(0).expect("No results?");
                        file_path.push('\0');
                        let mut file_rename: String = row.as_mut().unwrap().unwrap().get(1).expect("No results?");
                        file_rename.push('\0');
                        let lock_file: usize = row.unwrap().unwrap().get(2).expect("No results?");
                        MAIN_LISTVIEW_RESULTS.push((file_path, file_rename, lock_file));
                    }
                }
                //
            } else if asked.cmd.starts_with("transfer_data_to_exif_browser_list") {
                let path_to_match = asked.cmd.get(34..).unwrap();
                let cmd=format!("SELECT tag, value FROM exif WHERE path='{path_to_match}';");

                let mut stmt = db
                    .prepare(&cmd)
                    .expect("Prepare statement on transfer_data_to_exif_browser_list failed.");

                unsafe {
                    let mut rows = stmt.query([]);
                    loop {
                        let mut row = rows.as_mut().expect("row in rows failed").next();
                        if row.as_mut().unwrap().is_none() {
                            break;
                        }
                        let mut exif_tag: String = row.as_mut().unwrap().unwrap().get(0).expect("No results?");
                        exif_tag.push('\0');
                        let mut exif_value: String = row.as_mut().unwrap().unwrap().get(1).expect("No results?");
                        exif_value.push('\0');
                        MAIN_LISTVIEW_RESULTS.push((exif_tag, exif_value, 0));
                    }
                }
                
            } else if asked.cmd.starts_with("Quit") {
                unsafe {
                    PostThreadMessageA(MAIN_THREAD_ID, WM_QUIT, WPARAM(2), LPARAM(0));
                }
                my_response = "".to_string();

                //
            } else {
                Fail!("Got an internal command I did not recognise.😥");
            }
            asked.tx.send(my_response).expect("Something went wrong in the database server\nwhile trying to send a responce back.");
        }
    } else {
        Fail!("Could not start internal database service. 😯");
    }
}

/// Shorthand function to make the code a little more readable
//
pub fn send_cmd(cmd: &str) -> String {
    let cmd = cmd.to_string();
    unsafe {
        let (tx2, rx2) = mpsc::channel();
        let xx = DBcommand { tx: tx2, cmd };
        let tx = RESULT_SENDER.as_ref().unwrap().lock().unwrap().clone();

        tx.send(xx).unwrap();
        rx2.recv().unwrap()
    }
}

/// Get an integer value from the settings database
pub fn GetIntSetting(id: i32) -> usize {
    let cmd = format!("GetIntSetting={}", id);
    send_cmd(&cmd).as_str().parse::<usize>().unwrap()
}

/// Set an integer value from the settings database
pub fn SetIntSetting(id: i32, value: isize) {
    let cmd = format!("SetIntSetting={}={}", id, value);
    send_cmd(&cmd);
}

/// Get a TEXT value from the settings database
pub fn GetTextSetting(id: i32) -> String {
    let cmd = format!("GetTextSetting={}", id);
    send_cmd(&cmd)
}

/// Set a TEXT value in the settings database
pub fn SetTextSetting(id: i32, value: String) {
    let cmd = format!("SetTextSetting={}={}", id, value);
    send_cmd(&cmd);
}

/// Wrapper function to reload settings database from disc
pub fn ReloadSettings() {
    send_cmd("ReloadSettings");
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
pub fn SaveSettings() {
    send_cmd("SaveSettings");
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
pub fn ApplySettings(hwnd: HWND) {
    unsafe {
        SetIntSetting(IDC_PREFS_ON_CONFLICT, SendDlgItemMessageA(hwnd, IDC_PREFS_ON_CONFLICT, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_ON_CONFLICT_ADD, SendDlgItemMessageA(hwnd, IDC_PREFS_ON_CONFLICT_ADD, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_ON_CONFLICT_NUM, SendDlgItemMessageA(hwnd, IDC_PREFS_ON_CONFLICT_NUM, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_DATE_SHOOT_PRIMARY, SendDlgItemMessageA(hwnd, IDC_PREFS_DATE_SHOOT_PRIMARY, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_DATE_SHOOT_SECONDARY, SendDlgItemMessageA(hwnd, IDC_PREFS_DATE_SHOOT_SECONDARY, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_DRAG_N_DROP, SendDlgItemMessageA(hwnd, IDC_PREFS_DRAG_N_DROP, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        SetIntSetting(IDC_PREFS_EXIF_Engine, SendDlgItemMessageA(hwnd, IDC_PREFS_EXIF_Engine, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0);
        let mut tmp_text: [u16; MAX_PATH as usize] = [0; MAX_PATH as usize];
        let len = GetWindowTextW(GetDlgItem(hwnd, IDC_PREFS_ExifToolPath), &mut tmp_text);
        let exif_tool_path = String::from_utf16_lossy(&tmp_text[..len as usize]);
        SetTextSetting(IDC_PREFS_ExifToolPath, exif_tool_path);
        SetIntSetting(IDC_PREFS_NX_STUDIO, IsDlgButtonChecked(hwnd, IDC_PREFS_NX_STUDIO).try_into().unwrap());
    }
}

/// Counts the number of <what>s in a <table> which resides in our in memory database
pub fn Count(what: &str, table: &str) -> usize {
    let cmd = format!("Count={}={}", what, table);
    send_cmd(&cmd).as_str().parse::<usize>().unwrap()
}

/// Gets file masks/patterns from our in memory database
pub fn GetFilePatterns(idx: usize, zName: &mut String, zSpec: &mut String) {
    let cmd = format!("GetFilePatterns={}", idx);
    let answer = send_cmd(&cmd);
    let delimeter = answer.rfind('&').unwrap();
    *zName = answer.get(..delimeter).unwrap().to_string();
    *zSpec = answer.get(delimeter + 1..).unwrap().to_string();
}

/// Gets file speccs from our in memory database
pub fn GetFileSpec(idx: usize, zSpec: &mut String) {
    let cmd = format!("GetFileSpec={}", idx);
    let answer = send_cmd(&cmd);
    *zSpec = answer;
}

/// Deletes a file masks/patterns from our in memory database
pub fn DeleteFilePattern(zName: &mut String) {
    let cmd = format!("DeleteFilePattern={}", zName);
    send_cmd(&cmd);
}

/// Makes a temporary copy of the file pattern table in our in-memory database
pub fn MakeTempFilePatternDatabase() {
    send_cmd("MakeTempFilePatternDatabase");
}

/// Restores the default file patterns
pub fn RestoreFilePatternDatabase() {
    send_cmd("RestoreFilePatternDatabase");
}

/// Gets file masks/patterns from our in memory database
pub fn AddFilePattern(idx: usize, zName: String, zSpec: String) {
    let cmd = format!("AddFilePattern={}|+|{}|$|{}", idx, zName, zSpec);
    send_cmd(&cmd);
}

/// Runs a non-returning batch sql script
pub fn QuickNonReturningSqlCommand(sql: String) {
    let cmd = format!("QuickNonReturningSqlCommand={}", sql);
    send_cmd(&cmd);
}

/// Deletes a single entry, and any associated exif tags, from our database
pub fn DeleteFromDatabase(filename: String){
    let cmd: String=format!("DELETE FROM exif WHERE path='{filename}';DELETE FROM files WHERE path='{filename}';");
    QuickNonReturningSqlCommand(cmd);
}