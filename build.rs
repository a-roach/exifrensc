extern crate embed_resource;

use chrono::prelude::Local;
use chrono::TimeZone;
use std::collections::HashMap;
use std::env::consts::{ARCH, OS};
use std::fs::File;
use std::io::{Read, Write};

fn main() {
    let majorversion = env!("CARGO_PKG_VERSION_MAJOR");
    let minorversion = env!("CARGO_PKG_VERSION_MINOR");
    let anniversary = env!("CARGO_PKG_VERSION_PATCH");
    let annaversionary = chrono::Local.ymd(anniversary[0..4].parse::<i32>().unwrap(), anniversary[4..6].parse::<u32>().unwrap(), anniversary[6..8].parse::<u32>().unwrap()).and_hms(0, 0, 0);
    let now = Local::now();
    let diff = now.signed_duration_since(annaversionary);
    let days = diff.num_days();
    let seconds = diff.num_seconds() - (days * 86400);
    let minutes = (diff.num_seconds() - (days * 86400)) / 60;
    let iso_8601 = now.format("%Y%m%D");

    /*
     * Get the version from cargo.toml, then make a version string we can promulgate throughout the program
     */
/* 
    let version_string = format!(
        "{} {} ({} build, {} [{}], {})",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        "BUILD_TYPE",
        OS,
        ARCH,
        Local::now().format("%d %b %Y, %T")
    );

 */    /*
     * Update the manifest.xml file with the current build version
     * We actually load up a manifest.xml.in file and just replace a string with the version string.
     * We load the whole thing into memory in one hit because it is such a small file.
     */
    let mut fr = File::open("src/manifest.xml.in").expect("Could not open manifext.xml.in");
    let mut body = String::new();

    fr.read_to_string(&mut body).expect("Unable to read manifext.xml.in");
    drop(fr);
    body = body.replace("$CARGO_PKG_VERSION", env!("CARGO_PKG_VERSION"));
    body = body.replace("$MAJORVERSION", majorversion);
    body = body.replace("$MINORVERSION", minorversion);
    body = body.replace("$DAYVERSION", &days.to_string());
    body = body.replace("$MINUTEVERSION", &minutes.to_string());
    body = body.replace("$SECONDVERSION", &seconds.to_string());
    body = body.replace("$ISO8601VERSION", &iso_8601.to_string());

    let mut output = File::create("src/manifest.xml").expect("Create file failed");
    output.write_all(body.as_bytes()).expect("Write failed");
    drop(output);

    /*
     * Update the version.h file with the current build version
     */
    let mut fr = File::open("src/version.in").expect("Could not open version.in");
    let mut body = String::new();

    fr.read_to_string(&mut body).expect("Unable to read version.in");
    drop(fr);
    body = body.replace("$CARGO_PKG_VERSION", env!("CARGO_PKG_VERSION"));
    body = body.replace("$MAJORVERSION", majorversion);
    body = body.replace("$MINORVERSION", minorversion);
    body = body.replace("$DAYVERSION", &days.to_string());
    body = body.replace("$MINUTEVERSION", &minutes.to_string());
    body = body.replace("$SECONDVERSION", &seconds.to_string());
    body = body.replace("$ISO8601VERSION", &iso_8601.to_string());

    let mut output = File::create("src/version.rc").expect("Create file failed");
    output.write_all(body.as_bytes()).expect("Write failed");
    drop(output);

    /*
     * Create constants which can link the resource stub (written in C by ResEdit) with the main Rust program
     *
     * Next bit of code will parse the include file created by ResEdit, looking for #defines, then put them into a
     * hash map with their defined value so that we might use them later on in a custom structure
     */

    let mut body = String::new();
    let mut defines = HashMap::new();
    let mut contains_if = 0;
    let mut fr = File::open("src/resource.h").expect("Could not open resource.h");
    fr.read_to_string(&mut body).expect("Unable to read resource.h");
    drop(fr);

    let lines = body.lines();

    // Unfortunately #defines can not have more tha 16 characters in them, the code below will truncate at 16 for reasons I don't understand :-(
    for row in lines {
        let mut identifier = "";

        if row.contains("#if") {
            contains_if += 1;
        } else if contains_if > 0 {
            if row.contains("#endif") {
                contains_if -= 1;
            }
        } else if row.contains("#define") {
            let mut start_of_value = 0;

            for param in row.trim()[8..].trim().split(" ") {
                if start_of_value == 0 {
                    identifier = param;
                }
                start_of_value += 1;
            }

            let value = row.trim()[start_of_value..].trim();
            defines.insert(identifier, value);
        }
    }

    /*
     * Now all of that is over, we will parse the resource file created by ResEdit, look for particular controls, extract
     * their location and dimensions, look to see if we have a value for the ID of the control, then we will create
     * a new structure to pass into rust.
     */

    let mut out_body = String::new();
    out_body.push_str(
r#"
// This is file is generated automatically by build.rs. Editing it will be futile!
//
// Structure which holds the id, location and dimensions of controls

pub struct ControlStuff
  { id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32
  }

"#,
    );

    let mut body = String::new();
    let mut fr = File::open("src/exifrensc_res.rc").expect("Could not open exifrensc_res.rc");
    fr.read_to_string(&mut body).expect("Unable to read exifrensc_res.rc");
    drop(fr);

    let lines = body.lines();

    for row in lines {
        let mut idx: usize = 0;
        let mut contains_pushbutton: bool = false;
        let mut contains_edittext: bool = false;
        let mut contains_control: bool = false;
        let mut define_string = String::new();
        let suffix="_R"; // if you define suffix as "" then no suffixes are appended and all the constants retain the original #define name. Change it to something else and the extra rectangle information is included

        if !row.contains("IDCANCEL") && !row.contains("IDOK") && !row.contains("IDC_STATIC") {
            for param in row.split(",") {
                if param.contains("PUSHBUTTON") || param.contains("LTEXT") || param.contains("CTEXT") || param.contains("RTEXT") || param.contains("GROUPBOX") {
                    contains_pushbutton = true;
                } else if param.contains("EDITTEXT") || param.contains("COMBOBOX") {
                    contains_edittext = true;
                } else if param.contains("CONTROL") {
                    contains_control = true;
                }

                if contains_pushbutton == true {
                    match idx {
                        1 => {
                            // the identifier == #define ?

                            match defines.get(param.trim()) {
                                Some(&text) => {
                                    define_string.push_str("pub const ");
                                    define_string.push_str(param.trim());
                                    define_string.push_str(suffix);
                                    define_string.push_str(": ControlStuff = ControlStuff{ id: ");
                                    define_string.push_str(text);
                                    if suffix =="" {defines.remove(param.trim());}
                                }
                                _ => println!("Errr… 🤨 {}", param.trim()),
                            }
                        }
                        2 => {
                            // left / x
                            define_string.push_str(", x:");
                            define_string.push_str(param);
                            define_string.push_str(", ");
                        }
                        3 => {
                            // top / y
                            define_string.push_str("y:");
                            define_string.push_str(param);
                            define_string.push_str(", ");
                        }
                        4 => {
                            // right / cx /  width
                            define_string.push_str("width:");
                            define_string.push_str(param);
                            define_string.push_str(", ");
                        }
                        5 => {
                            // bottom / cy / height
                            define_string.push_str("height:");
                            define_string.push_str(param);
                            define_string.push_str("};");
                        }
                        _ => (),
                    }
                } else if contains_edittext == true {
                    match idx {
                        0 => {
                            // the identifier == #define ?

                            match defines.get((&param.trim()[9..]).trim()) {
                                Some(&text) => {
                                    define_string.push_str("pub const ");
                                    define_string.push_str((&param.trim()[9..]).trim());
                                    define_string.push_str(suffix);
                                    define_string.push_str(":ControlStuff = ControlStuff{ id: ");
                                    define_string.push_str(text);
                                    if suffix =="" {defines.remove(param.trim());}
                                }
                                _ => println!("Errr… 🤨 {}", param.trim()),
                            }
                        }
                        1 => {
                            // left / x
                            define_string.push_str(", x:");
                            define_string.push_str(param);
                            define_string.push_str(", ");
                        }
                        2 => {
                            // top / y
                            define_string.push_str("y:");
                            define_string.push_str(param);
                            define_string.push_str(", ");
                        }
                        3 => {
                            // right / cx /  width
                            define_string.push_str("width:");
                            define_string.push_str(param);
                            define_string.push_str(", ");
                        }
                        4 => {
                            // bottom / cy / height
                            define_string.push_str("height:");
                            define_string.push_str(param);
                            define_string.push_str("};");
                        }
                        _ => (),
                    }
                } else if contains_control == true {
                    match idx {
                        1 => match defines.get(param.trim()) {
                            Some(&text) => {
                                define_string.push_str("pub const ");
                                define_string.push_str(param.trim());
                                define_string.push_str(suffix);
                                define_string.push_str(":ControlStuff = ControlStuff{ id: ");
                                define_string.push_str(text);
                                if suffix =="" {defines.remove(param.trim());}
                            }
                            _ => println!("Errr… 🤨 {}", param.trim()),
                        },

                        4 => {
                            // left / x
                            define_string.push_str(", x:");
                            define_string.push_str(param);
                            define_string.push_str(", ");
                        }
                        5 => {
                            // top / y
                            define_string.push_str("y:");
                            define_string.push_str(param);
                            define_string.push_str(", ");
                        }
                        6 => {
                            // right / cx /  width
                            define_string.push_str("width:");
                            define_string.push_str(param);
                            define_string.push_str(", ");
                        }
                        7 => {
                            // bottom / cy / height
                            define_string.push_str("height:");
                            define_string.push_str(param);
                            define_string.push_str("};");
                        }
                        _ => (),
                    }
                }
                idx += 1;
            }
        }

        if define_string != "" && suffix !="" {
            out_body.push_str(&define_string);
            out_body.push_str("\n");
        };
    }

    /*
     * Walk through the left over defines and add them to the file
     */
    for (identifier, val) in defines.iter() {
        out_body.push_str(&format!("pub const {}: i32 = {};\n", identifier, val));
    }

    /*
     * Save the "include" file to disk
     */
    let mut output = File::create("src/resource_defs.rs").expect("Create file failed");
    output.write_all(out_body.as_bytes()).expect("Write failed");
    drop(output);

    /*
     * Compile and link in our resource file
     */
    embed_resource::compile("src/exifrensc_res.rc");
}
