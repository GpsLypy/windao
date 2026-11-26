use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

use eframe::egui::{self, RichText, Ui};
use windows::{
    Devices::{
        Bluetooth::{
            BluetoothCacheMode, BluetoothDevice, BluetoothLEDevice,
            GenericAttributeProfile::{
                GattCharacteristicUuids, GattCommunicationStatus, GattServiceUuids,
            },
        },
        Enumeration::{
            DeviceAccessInformation, DeviceAccessStatus, DeviceInformation,
            DeviceInformationCollection, DeviceInformationKind,
        },
    },
    Foundation::IPropertyValue,
    Storage::Streams::DataReader,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::{HSTRING, Interface, RuntimeType},
};
use windows_collections::IIterable;
use windows_future::{AsyncOperationCompletedHandler, IAsyncOperation};

use super::{card, title};

const CONNECTED: &str = "System.Devices.Aep.IsConnected";
const ADDRESS: &str = "System.Devices.Aep.DeviceAddress";
const TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Default)]
pub(super) struct Wireless {
    devices: Vec<Device>,
    scanned: bool,
    error: Option<ReadError>,
    settings_error: bool,
    pending: Option<Receiver<Update>>,
}

struct Device {
    id: HSTRING,
    address: Option<String>,
    name: String,
    connected: Option<bool>,
    le: bool,
    battery: Result<Option<u8>, ReadError>,
}

#[derive(Clone, Copy)]
enum ReadError {
    Denied,
    TimedOut,
    Unavailable,
}

impl ReadError {
    fn message(self) -> &'static str {
        match self {
            Self::Denied => "Windows 拒绝访问蓝牙设备。请检查蓝牙访问权限。",
            Self::TimedOut => "读取超时。请确认设备在附近，然后重试。",
            Self::Unavailable => "Windows 暂时无法读取设备。请检查蓝牙设置后重试。",
        }
    }
}

impl From<windows::core::Error> for ReadError {
    fn from(error: windows::core::Error) -> Self {
        if error.code().0 as u32 == 0x80070005 {
            Self::Denied
        } else {
            Self::Unavailable
        }
    }
}

enum Update {
    Devices(Vec<Device>, Option<ReadError>),
    Battery(HSTRING, Result<Option<u8>, ReadError>),
}

impl Wireless {
    pub fn show(&mut self, ui: &mut Ui) {
        self.poll();
        title(ui, "无线设备");
        let busy = self.pending.is_some();
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            refresh = ui.add_enabled(!busy, egui::Button::new("刷新")).clicked();
            if ui.button("蓝牙设置").clicked() {
                self.settings_error = !crate::windows::open_bluetooth_settings();
            }
            if busy {
                ui.spinner();
                ui.weak("正在读取");
            }
        });
        if self.settings_error {
            ui.colored_label(ui.visuals().error_fg_color, "无法打开 Windows 蓝牙设置。");
        }
        if !busy && (refresh || !self.scanned) {
            self.scanned = true;
            self.error = None;
            self.start(ui.ctx(), || {
                scan([paired_devices(true), paired_devices(false)])
            });
        }
        let busy = self.pending.is_some();
        if let Some(error) = self.error {
            ui.colored_label(ui.visuals().error_fg_color, error.message());
        } else if self.scanned && self.devices.is_empty() && self.pending.is_none() {
            card(ui, |ui| {
                ui.label("没有已配对的蓝牙设备");
                ui.weak("在 Windows 蓝牙设置中配对设备后，点击刷新。");
            });
        }
        let mut battery_id = None;
        for device in &self.devices {
            card(ui, |ui| {
                ui.label(RichText::new(&device.name).strong());
                ui.horizontal_wrapped(|ui| {
                    ui.label(match device.connected {
                        Some(true) => "已连接",
                        Some(false) => "已断开",
                        None => "连接状态未知",
                    });
                    match device.battery {
                        Ok(Some(percent)) => {
                            ui.label(format!("电量 {percent}%"));
                        }
                        Ok(None) => {
                            ui.weak("电量未知");
                        }
                        Err(error) => {
                            ui.colored_label(ui.visuals().error_fg_color, error.message());
                        }
                    }
                    if device.le
                        && device.connected != Some(false)
                        && ui
                            .add_enabled(!busy, egui::Button::new("读取电量"))
                            .clicked()
                    {
                        battery_id = Some(device.id.clone());
                    }
                });
            });
        }
        if let Some(id) = battery_id {
            // Start FromIdAsync on the UI thread so Windows can show a consent prompt.
            match BluetoothLEDevice::FromIdAsync(&id) {
                Ok(operation) => self.start(ui.ctx(), move || {
                    let battery = match wait(operation) {
                        Ok(device) => check_access(&id).and_then(|()| read_battery(&device)),
                        Err(error) => match check_access(&id) {
                            Err(ReadError::Denied) => Err(ReadError::Denied),
                            _ => Err(error),
                        },
                    };
                    Update::Battery(id, battery)
                }),
                Err(error) => self.apply(Update::Battery(id, Err(error.into()))),
            }
        }
    }

    fn start(&mut self, ctx: &egui::Context, work: impl FnOnce() -> Update + Send + 'static) {
        let (send, receive) = mpsc::sync_channel(1);
        let ctx = ctx.clone();
        let worker = thread::Builder::new()
            .name("windao-bluetooth".into())
            .spawn(move || {
                if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok() {
                    let _ = send.send(work());
                    unsafe { RoUninitialize() };
                } else {
                    drop(send);
                }
                ctx.request_repaint();
            });
        match worker {
            Ok(_) => self.pending = Some(receive),
            Err(_) => self.error = Some(ReadError::Unavailable),
        }
    }

    fn poll(&mut self) {
        let Some(receiver) = &self.pending else {
            return;
        };
        match receiver.try_recv() {
            Ok(update) => {
                self.pending = None;
                self.apply(update);
            }
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.error = Some(ReadError::Unavailable);
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    fn apply(&mut self, update: Update) {
        match update {
            Update::Devices(devices, error) => {
                if !devices.is_empty() || error.is_none() {
                    self.devices = devices;
                }
                self.error = error;
            }
            Update::Battery(id, battery) => {
                if let Some(device) = self.devices.iter_mut().find(|device| device.id == id) {
                    device.battery = battery;
                }
            }
        }
    }
}

fn paired_devices(le: bool) -> Result<IAsyncOperation<DeviceInformationCollection>, ReadError> {
    let selector = if le {
        BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)?
    } else {
        BluetoothDevice::GetDeviceSelectorFromPairingState(true)?
    };
    let properties =
        IIterable::<HSTRING>::from(vec![HSTRING::from(CONNECTED), HSTRING::from(ADDRESS)]);
    Ok(
        DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(
            &selector,
            &properties,
            DeviceInformationKind::AssociationEndpoint,
        )?,
    )
}

fn scan(
    operations: [Result<IAsyncOperation<DeviceInformationCollection>, ReadError>; 2],
) -> Update {
    let mut devices: Vec<Device> = Vec::new();
    let mut error = None;
    for (is_le, operation) in [true, false].into_iter().zip(operations) {
        match operation.and_then(wait) {
            Ok(results) => {
                for information in results {
                    let Some(device) = read_endpoint(information, is_le) else {
                        continue;
                    };
                    if let Some(existing) = devices.iter_mut().find(|existing| {
                        existing.id == device.id
                            || device
                                .address
                                .as_ref()
                                .is_some_and(|address| existing.address.as_ref() == Some(address))
                    }) {
                        if existing.connected != Some(true) {
                            existing.connected = device.connected.or(existing.connected);
                        }
                    } else {
                        devices.push(device);
                    }
                }
            }
            Err(found) => {
                if error.is_none() || matches!(found, ReadError::Denied) {
                    error = Some(found);
                }
            }
        }
    }
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.name.cmp(&b.name))
    });
    Update::Devices(devices, error)
}

pub(crate) fn connected_devices() -> Option<std::collections::BTreeMap<String, String>> {
    let Update::Devices(devices, None) = scan([paired_devices(true), paired_devices(false)]) else {
        return None;
    };
    let mut connected = std::collections::BTreeMap::new();
    for device in devices {
        if device.connected? {
            connected.insert(
                device.address.unwrap_or_else(|| device.id.to_string()),
                device.name,
            );
        }
    }
    Some(connected)
}

fn read_endpoint(info: DeviceInformation, le: bool) -> Option<Device> {
    let id = info.Id().ok()?;
    let properties = info.Properties().ok();
    let property = |key: &str| {
        properties
            .as_ref()?
            .Lookup(&HSTRING::from(key))
            .ok()?
            .cast::<IPropertyValue>()
            .ok()
    };
    let connected = property(CONNECTED).and_then(|value| value.GetBoolean().ok());
    let address = property(ADDRESS)
        .and_then(|value| value.GetString().ok())
        .map(|value| value.to_string().to_ascii_lowercase())
        .filter(|address| !address.is_empty());
    let name = info
        .Name()
        .map(|value| value.to_string())
        .unwrap_or_default();
    Some(Device {
        id,
        address,
        name: if name.trim().is_empty() {
            "未命名设备".into()
        } else {
            name
        },
        connected,
        le,
        battery: Ok(None),
    })
}

fn wait<T: RuntimeType + 'static>(operation: IAsyncOperation<T>) -> Result<T, ReadError> {
    let (send, receive) = mpsc::sync_channel(1);
    operation.SetCompleted(&AsyncOperationCompletedHandler::new(move |_, _| {
        let _ = send.try_send(());
        Ok(())
    }))?;
    if receive.recv_timeout(TIMEOUT).is_err() {
        let _ = operation.Cancel();
        return Err(ReadError::TimedOut);
    }
    Ok(operation.GetResults()?)
}

fn check_access(id: &HSTRING) -> Result<(), ReadError> {
    let status = DeviceAccessInformation::CreateFromId(id)?.CurrentStatus()?;
    if status == DeviceAccessStatus::DeniedBySystem || status == DeviceAccessStatus::DeniedByUser {
        Err(ReadError::Denied)
    } else {
        Ok(())
    }
}

fn read_battery(device: &BluetoothLEDevice) -> Result<Option<u8>, ReadError> {
    let services = wait(device.GetGattServicesForUuidWithCacheModeAsync(
        GattServiceUuids::Battery()?,
        BluetoothCacheMode::Uncached,
    )?)?;
    if !gatt_success(services.Status()?)? {
        return Ok(None);
    }
    // A device can expose several battery services. A single percentage is shown
    // only when it reports one standard Battery Level characteristic.
    let services = services.Services()?;
    if services.Size()? != 1 {
        return Ok(None);
    }
    let service = services.GetAt(0)?;
    let characteristics = wait(service.GetCharacteristicsForUuidWithCacheModeAsync(
        GattCharacteristicUuids::BatteryLevel()?,
        BluetoothCacheMode::Cached,
    )?)?;
    if !gatt_success(characteristics.Status()?)? {
        return Ok(None);
    }
    let characteristics = characteristics.Characteristics()?;
    if characteristics.Size()? != 1 {
        return Ok(None);
    }
    let result = wait(
        characteristics
            .GetAt(0)?
            .ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
    )?;
    if !gatt_success(result.Status()?)? {
        return Ok(None);
    }
    let reader = DataReader::FromBuffer(&result.Value()?)?;
    if reader.UnconsumedBufferLength()? != 1 {
        return Ok(None);
    }
    let percent = reader.ReadByte()?;
    Ok((percent <= 100).then_some(percent))
}

fn gatt_success(status: GattCommunicationStatus) -> Result<bool, ReadError> {
    if status == GattCommunicationStatus::AccessDenied {
        Err(ReadError::Denied)
    } else {
        Ok(status == GattCommunicationStatus::Success)
    }
}
