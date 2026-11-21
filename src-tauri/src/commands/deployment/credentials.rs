#[cfg(windows)]
const TARGET: &str = "CEZECORE.Vercel.ApiToken";

#[cfg(windows)]
fn target_wide() -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(TARGET)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
pub fn read_token() -> Result<Option<String>, String> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC},
    };

    let target = target_wide();
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_NOT_FOUND {
            return Ok(None);
        }
        return Err(format!(
            "Windows Credential Manager could not read the Vercel token (error {error})."
        ));
    }

    let result = unsafe {
        let length = (*credential).CredentialBlobSize as usize;
        if length == 0 || (*credential).CredentialBlob.is_null() {
            Err("The saved Vercel credential is empty. Reconnect your account.".to_string())
        } else {
            let blob = std::slice::from_raw_parts((*credential).CredentialBlob, length);
            String::from_utf8(blob.to_vec()).map_err(|_| {
                "The saved Vercel credential is unreadable. Reconnect your account.".to_string()
            })
        }
    };
    unsafe { CredFree(credential.cast()) };
    result.map(Some)
}

#[cfg(not(windows))]
pub fn read_token() -> Result<Option<String>, String> {
    Err(
        "Vercel credentials are currently supported only in Windows Credential Manager."
            .to_string(),
    )
}

#[cfg(windows)]
pub fn write_token(token: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::GetLastError,
        Security::Credentials::{
            CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        },
    };

    if token.len() > 5_120 {
        return Err("The Vercel API token exceeds the Windows credential size limit.".to_string());
    }
    let target = target_wide();
    let username: Vec<u16> = std::ffi::OsStr::new("Vercel API token")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut credential = CREDENTIALW::default();
    credential.Type = CRED_TYPE_GENERIC;
    credential.TargetName = target.as_ptr() as *mut u16;
    credential.UserName = username.as_ptr() as *mut u16;
    credential.CredentialBlob = token.as_ptr() as *mut u8;
    credential.CredentialBlobSize = token.len() as u32;
    credential.Persist = CRED_PERSIST_LOCAL_MACHINE;

    if unsafe { CredWriteW(&credential, 0) } == 0 {
        return Err(format!(
            "Windows Credential Manager could not save the Vercel token (error {}).",
            unsafe { GetLastError() }
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn write_token(_token: &str) -> Result<(), String> {
    Err(
        "Vercel credentials are currently supported only in Windows Credential Manager."
            .to_string(),
    )
}

#[cfg(windows)]
pub fn delete_token() -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC},
    };

    let target = target_wide();
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_NOT_FOUND {
            return Ok(());
        }
        return Err(format!(
            "Windows Credential Manager could not remove the Vercel token (error {error})."
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn delete_token() -> Result<(), String> {
    Err(
        "Vercel credentials are currently supported only in Windows Credential Manager."
            .to_string(),
    )
}
