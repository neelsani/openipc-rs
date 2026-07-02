//! Android `NativeActivity` and `UsbManager` integration.

use std::{
    os::fd::{FromRawFd as _, OwnedFd},
    sync::{Condvar, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use android_activity::AndroidApp;
use jni::{
    jni_sig, jni_str,
    objects::{Global, JByteArray, JClass, JObject, JString},
    vm::JavaVM,
    EnvUnowned, JValue,
};
use nusb::MaybeFuture as _;

use crate::runtime::UsbDeviceInfo;

static ANDROID_APP: OnceLock<Mutex<Option<AndroidApp>>> = OnceLock::new();
static KEY_FILE_RESULT: OnceLock<Mutex<Option<Result<SelectedKeyFile, String>>>> = OnceLock::new();
static KEY_FILE_CONTEXT: OnceLock<Mutex<Option<eframe::egui::Context>>> = OnceLock::new();
#[derive(Default)]
struct VpnRequestState {
    waiting: bool,
    result: Option<Result<OpenedVpn, String>>,
}

type VpnResult = (Mutex<VpnRequestState>, Condvar);
static VPN_RESULT: OnceLock<VpnResult> = OnceLock::new();
const USB_PERMISSION_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct SelectedKeyFile {
    pub(crate) name: String,
    pub(crate) bytes: Vec<u8>,
}

/// USB device and Java connection retained for the lifetime of `nusb` access.
pub(crate) struct OpenedUsbDevice {
    pub(crate) device: nusb::Device,
    pub(crate) info: UsbDeviceInfo,
    pub(crate) connection: Global<JObject<'static>>,
}

pub(crate) struct OpenedVpn {
    pub(crate) fd: i32,
    pub(crate) interface_name: String,
}

pub(crate) fn install(app: AndroidApp) {
    *ANDROID_APP
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("Android app mutex poisoned") = Some(app);
}

pub(crate) fn open_key_file(context: eframe::egui::Context) -> Result<(), String> {
    *KEY_FILE_CONTEXT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("Android key picker context mutex poisoned") = Some(context);
    let app = app()?;
    let vm = java_vm(&app)?;
    vm.attach_current_thread(|env| {
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: android-activity owns this global activity reference for the
        // lifetime of the cloned AndroidApp handle.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        env.call_method(&activity, jni_str!("openKeyFile"), jni_sig!("()V"), &[])?;
        Ok(())
    })
    .map_err(|error: jni::errors::Error| format!("Android key picker failed: {error}"))
}

pub(crate) fn take_key_file_result() -> Option<Result<SelectedKeyFile, String>> {
    KEY_FILE_RESULT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()?
        .take()
}

pub(crate) fn open_vpn() -> Result<OpenedVpn, String> {
    let state = VPN_RESULT.get_or_init(|| (Mutex::new(VpnRequestState::default()), Condvar::new()));
    let app = app()?;
    let vm = java_vm(&app)?;
    {
        let mut request = state.0.lock().expect("Android VPN result mutex poisoned");
        if request.waiting {
            return Err("Android VPN permission request is already active".to_owned());
        }
        request.waiting = true;
        request.result = None;
    }
    let request_result: Result<(), jni::errors::Error> = vm.attach_current_thread(|env| {
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: android-activity owns the activity for the lifetime of `app`.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        env.call_method(&activity, jni_str!("openVpn"), jni_sig!("()V"), &[])?;
        Ok(())
    });
    if let Err(error) = request_result {
        state
            .0
            .lock()
            .expect("Android VPN result mutex poisoned")
            .waiting = false;
        return Err(format!("Android VPN request failed: {error}"));
    }

    let mut request = state
        .1
        .wait_timeout_while(
            state.0.lock().expect("Android VPN result mutex poisoned"),
            USB_PERMISSION_TIMEOUT,
            |request| request.result.is_none(),
        )
        .map_err(|_| "Android VPN result mutex poisoned".to_owned())?
        .0;
    request.waiting = false;
    let result = request.result.take();
    result.unwrap_or_else(|| Err("Android VPN permission timed out".to_owned()))
}

pub(crate) fn close_vpn(fd: i32) {
    let Ok(app) = app() else {
        return;
    };
    let Ok(vm) = java_vm(&app) else {
        return;
    };
    let _: Result<(), jni::errors::Error> = vm.attach_current_thread(|env| {
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: android-activity owns the activity for the lifetime of `app`.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        env.call_method(
            &activity,
            jni_str!("closeVpn"),
            jni_sig!("(I)V"),
            &[JValue::Int(fd)],
        )?;
        Ok(())
    });
}

fn finish_key_file(result: Result<SelectedKeyFile, String>) {
    if let Ok(mut pending) = KEY_FILE_RESULT.get_or_init(|| Mutex::new(None)).lock() {
        *pending = Some(result);
    }
    if let Some(context) = KEY_FILE_CONTEXT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut context| context.take())
    {
        context.request_repaint();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_neels_openipc_nebulus_NebulusActivity_nativeKeySelected<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    name: JString<'local>,
    bytes: JByteArray<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            finish_key_file(Ok(SelectedKeyFile {
                name: name.try_to_string(env)?,
                bytes: env.convert_byte_array(&bytes)?,
            }));
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_neels_openipc_nebulus_NebulusActivity_nativeKeyError<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    message: JString<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            finish_key_file(Err(message.try_to_string(env)?));
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_neels_openipc_nebulus_NebulusActivity_nativeVpnOpened<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    fd: i32,
    interface_name: JString<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            let name = interface_name.try_to_string(env)?;
            let state =
                VPN_RESULT.get_or_init(|| (Mutex::new(VpnRequestState::default()), Condvar::new()));
            let accepted = {
                let mut request = state.0.lock().expect("Android VPN result mutex poisoned");
                if request.waiting {
                    request.result = Some(Ok(OpenedVpn {
                        fd,
                        interface_name: name,
                    }));
                    true
                } else {
                    false
                }
            };
            if accepted {
                state.1.notify_all();
            } else {
                close_vpn(fd);
            }
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_neels_openipc_nebulus_NebulusActivity_nativeVpnError<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    message: JString<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            let message = message.try_to_string(env)?;
            let state =
                VPN_RESULT.get_or_init(|| (Mutex::new(VpnRequestState::default()), Condvar::new()));
            let mut request = state.0.lock().expect("Android VPN result mutex poisoned");
            if request.waiting {
                request.result = Some(Err(message));
                state.1.notify_all();
            }
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

fn app() -> Result<AndroidApp, String> {
    ANDROID_APP
        .get()
        .and_then(|app| app.lock().ok()?.clone())
        .ok_or_else(|| "Android activity is not ready".to_owned())
}

pub(crate) fn list_devices() -> Result<Vec<UsbDeviceInfo>, String> {
    let app = app()?;
    let vm = java_vm(&app)?;
    vm.attach_current_thread(|env| {
        let manager = usb_manager(env, &app)?;
        let iterator = device_iterator(env, &manager)?;
        let mut devices = Vec::new();
        while env
            .call_method(&iterator, jni_str!("hasNext"), jni_sig!("()Z"), &[])?
            .z()?
        {
            let device = env
                .call_method(
                    &iterator,
                    jni_str!("next"),
                    jni_sig!("()Ljava/lang/Object;"),
                    &[],
                )?
                .l()?;
            if let Some(info) = device_info(env, &device)? {
                devices.push(info);
            }
        }
        Ok(devices)
    })
    .map_err(|error: jni::errors::Error| format!("Android USB discovery failed: {error}"))
}

/// Ask Android for access when needed and convert its connection fd to nusb.
pub(crate) fn open_device(selected: Option<&str>) -> Result<OpenedUsbDevice, String> {
    let app = app()?;
    let vm = java_vm(&app)?;
    let selected = selected.and_then(parse_device_id);
    let (manager, device, info, permission_granted) = vm
        .attach_current_thread(|env| {
            let manager = usb_manager(env, &app)?;
            let iterator = device_iterator(env, &manager)?;
            let mut found = None;
            while env
                .call_method(&iterator, jni_str!("hasNext"), jni_sig!("()Z"), &[])?
                .z()?
            {
                let device = env
                    .call_method(
                        &iterator,
                        jni_str!("next"),
                        jni_sig!("()Ljava/lang/Object;"),
                        &[],
                    )?
                    .l()?;
                let Some(info) = device_info(env, &device)? else {
                    continue;
                };
                let matches = selected
                    .map(|ids| ids == (info.vendor_id, info.product_id))
                    .unwrap_or(true);
                if matches {
                    found = Some((device, info));
                    break;
                }
            }
            let (device, info) = found.ok_or_else(|| {
                jni::errors::Error::NullPtr("No supported Realtek USB adapter is attached")
            })?;
            let granted = has_permission(env, &manager, &device)?;
            if !granted {
                request_permission(env, &app, &manager, &device)?;
            }
            Ok((
                env.new_global_ref(manager)?,
                env.new_global_ref(device)?,
                info,
                granted,
            ))
        })
        .map_err(|error: jni::errors::Error| format!("Android USB open failed: {error}"))?;

    if !permission_granted {
        let started = Instant::now();
        loop {
            let granted = vm
                .attach_current_thread(|env| has_permission(env, &manager, &device))
                .map_err(|error: jni::errors::Error| {
                    format!("Android USB permission check failed: {error}")
                })?;
            if granted {
                break;
            }
            if started.elapsed() >= USB_PERMISSION_TIMEOUT {
                return Err("Android USB permission was not granted".to_owned());
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    let (fd, connection) = vm
        .attach_current_thread(|env| {
            let connection = env
                .call_method(
                    &manager,
                    jni_str!("openDevice"),
                    jni_sig!("(Landroid/hardware/usb/UsbDevice;)Landroid/hardware/usb/UsbDeviceConnection;"),
                    &[JValue::Object(device.as_ref())],
                )?
                .check_null()?
                .l()?;
            let fd = env
                .call_method(
                    &connection,
                    jni_str!("getFileDescriptor"),
                    jni_sig!("()I"),
                    &[],
                )?
                .i()?;
            Ok((fd, env.new_global_ref(connection)?))
        })
        .map_err(|error: jni::errors::Error| format!("Android USB open failed: {error}"))?;
    let owned_fd = duplicate_fd(fd)?;
    let device = nusb::Device::from_fd(owned_fd)
        .wait()
        .map_err(|error| format!("nusb could not open Android USB fd: {error}"))?;
    Ok(OpenedUsbDevice {
        device,
        info,
        connection,
    })
}

fn java_vm(app: &AndroidApp) -> Result<JavaVM, String> {
    // SAFETY: AndroidActivity owns this VM for at least as long as its cloned
    // AndroidApp handle. JavaVM is a non-owning process-wide handle.
    Ok(unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) })
}

fn usb_manager<'local>(
    env: &mut jni::Env<'local>,
    app: &AndroidApp,
) -> jni::errors::Result<JObject<'local>> {
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
    // SAFETY: android-activity documents this pointer as an unowned global
    // reference valid for the lifetime of `app`; Cast does not delete it.
    let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
    let service = env.new_string("usb")?;
    env.call_method(
        &activity,
        jni_str!("getSystemService"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(service.as_ref())],
    )?
    .check_null()?
    .l()
}

fn device_iterator<'local>(
    env: &mut jni::Env<'local>,
    manager: &JObject<'local>,
) -> jni::errors::Result<JObject<'local>> {
    let map = env
        .call_method(
            manager,
            jni_str!("getDeviceList"),
            jni_sig!("()Ljava/util/HashMap;"),
            &[],
        )?
        .check_null()?
        .l()?;
    let values = env
        .call_method(
            &map,
            jni_str!("values"),
            jni_sig!("()Ljava/util/Collection;"),
            &[],
        )?
        .check_null()?
        .l()?;
    env.call_method(
        &values,
        jni_str!("iterator"),
        jni_sig!("()Ljava/util/Iterator;"),
        &[],
    )?
    .check_null()?
    .l()
}

fn device_info(
    env: &mut jni::Env<'_>,
    device: &JObject<'_>,
) -> jni::errors::Result<Option<UsbDeviceInfo>> {
    let vendor_id = env
        .call_method(device, jni_str!("getVendorId"), jni_sig!("()I"), &[])?
        .i()? as u16;
    let product_id = env
        .call_method(device, jni_str!("getProductId"), jni_sig!("()I"), &[])?
        .i()? as u16;
    let Some(supported) = openipc_rtl88xx::supported_device(vendor_id, product_id) else {
        return Ok(None);
    };
    Ok(Some(UsbDeviceInfo {
        id: format!("{vendor_id:04x}:{product_id:04x}"),
        label: supported.label.to_owned(),
        vendor_id,
        product_id,
    }))
}

fn has_permission<'local, 'object>(
    env: &mut jni::Env<'local>,
    manager: &impl AsRef<JObject<'object>>,
    device: &impl AsRef<JObject<'object>>,
) -> jni::errors::Result<bool> {
    env.call_method(
        manager,
        jni_str!("hasPermission"),
        jni_sig!("(Landroid/hardware/usb/UsbDevice;)Z"),
        &[JValue::Object(device.as_ref())],
    )?
    .z()
}

fn request_permission(
    env: &mut jni::Env<'_>,
    app: &AndroidApp,
    manager: &JObject<'_>,
    device: &JObject<'_>,
) -> jni::errors::Result<()> {
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
    // SAFETY: See `usb_manager`; this is another scoped borrow of the same
    // android-activity-owned global reference.
    let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
    let action = env.new_string("dev.neels.openipc.nebulus.USB_PERMISSION")?;
    let intent_class = env.find_class(jni_str!("android/content/Intent"))?;
    let intent = env.new_object(
        &intent_class,
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(action.as_ref())],
    )?;
    let pending_class = env.find_class(jni_str!("android/app/PendingIntent"))?;
    const FLAG_UPDATE_CURRENT: i32 = 0x0800_0000;
    const FLAG_MUTABLE: i32 = 0x0200_0000;
    let pending = env
        .call_static_method(
            &pending_class,
            jni_str!("getBroadcast"),
            jni_sig!(
                "(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;"
            ),
            &[
                JValue::Object(activity.as_ref()),
                JValue::Int(0),
                JValue::Object(&intent),
                JValue::Int(FLAG_UPDATE_CURRENT | FLAG_MUTABLE),
            ],
        )?
        .check_null()?
        .l()?;
    env.call_method(
        manager,
        jni_str!("requestPermission"),
        jni_sig!("(Landroid/hardware/usb/UsbDevice;Landroid/app/PendingIntent;)V"),
        &[JValue::Object(device), JValue::Object(&pending)],
    )?;
    Ok(())
}

fn duplicate_fd(fd: i32) -> Result<OwnedFd, String> {
    if fd < 0 {
        return Err(format!("Android returned invalid USB fd {fd}"));
    }
    // SAFETY: `fd` comes from a live UsbDeviceConnection. `dup` creates an
    // independent descriptor that can be transferred to nusb.
    let duplicate = unsafe { libc::dup(fd) };
    if duplicate < 0 {
        return Err(format!(
            "duplicate Android USB fd failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `dup` returned a fresh descriptor now exclusively owned here.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}

fn parse_device_id(value: &str) -> Option<(u16, u16)> {
    let (vendor, product) = value.split_once(':')?;
    Some((
        u16::from_str_radix(vendor, 16).ok()?,
        u16::from_str_radix(product, 16).ok()?,
    ))
}
