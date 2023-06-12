macro_rules! Warning {
    ($a:expr) => {
        MessageBoxA(None, s!($a), s!("Warning!"), MB_OK | MB_ICONINFORMATION);
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

macro_rules! Commit {
    () => {
        send_cmd("Commit");
    };
}

macro_rules! Begin {
    () => {
        send_cmd("Begin");
    };
}
