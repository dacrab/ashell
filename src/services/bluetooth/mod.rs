use super::impl_service_subscription;
use super::rfkill;
use super::{ReadOnlyService, Service, ServiceEvent};
use dbus::{BatteryProxy, BluetoothDbus, DeviceProxy};
use iced::{
    Task,
    futures::{SinkExt, Stream, StreamExt, channel::mpsc::Sender, stream_select},
};
use log::{debug, error, info, warn};
use std::{ops::Deref, pin::Pin, time::Duration};
use tokio::time::sleep;
use zbus::zvariant::OwnedObjectPath;

mod dbus;

type EventStream = Pin<Box<dyn Stream<Item = ()> + Send>>;

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum BluetoothState {
    Unavailable,
    Active,
    Inactive,
}

#[derive(Debug, Clone)]
pub struct BluetoothDevice {
    pub name: String,
    pub battery: Option<u8>,
    pub path: OwnedObjectPath,
    pub connected: bool,
    pub paired: bool,
}

#[derive(Debug, Clone)]
pub struct BluetoothData {
    pub state: BluetoothState,
    pub devices: Vec<BluetoothDevice>,
    pub discovering: bool,
}

#[derive(Debug, Clone)]
pub struct BluetoothService {
    conn: zbus::Connection,
    data: BluetoothData,
}

impl Deref for BluetoothService {
    type Target = BluetoothData;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

#[derive(Debug, Clone)]
pub enum BluetoothCommand {
    Toggle,
    StartDiscovery,
    PairDevice(OwnedObjectPath),
    ConnectDevice(OwnedObjectPath),
    DisconnectDevice(OwnedObjectPath),
    RemoveDevice(OwnedObjectPath),
}

enum State {
    Init,
    Active(zbus::Connection),
    Error,
}

impl BluetoothService {
    async fn initialize_data(conn: &zbus::Connection) -> anyhow::Result<BluetoothData> {
        let bluetooth = BluetoothDbus::new(conn).await?;

        let state = bluetooth.state().await?;
        let rfkill_soft_block = rfkill::check_soft_block().await?;

        let state = match state {
            BluetoothState::Unavailable => BluetoothState::Unavailable,
            BluetoothState::Active if rfkill_soft_block => BluetoothState::Inactive,
            state => state,
        };
        let devices = bluetooth.devices().await?;
        let discovering = bluetooth.discovering().await.unwrap_or(false);

        Ok(BluetoothData {
            state,
            devices,
            discovering,
        })
    }

    async fn events(conn: &zbus::Connection) -> anyhow::Result<impl Stream<Item = ()> + use<>> {
        let bluetooth = BluetoothDbus::new(conn).await?;

        let interface_changed = stream_select!(
            bluetooth
                .bluez
                .receive_interfaces_added()
                .await?
                .map(|_| {}),
            bluetooth
                .bluez
                .receive_interfaces_removed()
                .await?
                .map(|_| {}),
        )
        .boxed();

        let combined = match bluetooth.adapter.as_ref() {
            Some(adapter) => {
                let powered = adapter.receive_powered_changed().await.map(|_| {});
                let discovering = adapter.receive_discovering_changed().await.map(|_| {});
                let rfkill = rfkill::listen_soft_block_changes().await?;
                let devices = bluetooth.devices().await?;

                let mut batteries: Vec<EventStream> = Vec::with_capacity(devices.len());
                let mut device_properties: Vec<EventStream> = Vec::with_capacity(devices.len());
                for device in devices {
                    let conn = bluetooth.bluez.inner().connection();

                    let battery = BatteryProxy::builder(conn)
                        .path(device.path.clone())?
                        .build()
                        .await?;
                    batteries.push(
                        battery
                            .receive_percentage_changed()
                            .await
                            .map(|_| {})
                            .boxed(),
                    );

                    let device_proxy = DeviceProxy::builder(conn)
                        .path(device.path)?
                        .build()
                        .await?;
                    let connected_changed: EventStream = device_proxy
                        .receive_connected_changed()
                        .await
                        .map(|_| {})
                        .boxed();
                    device_properties.push(connected_changed);
                }

                let battery_events = if batteries.is_empty() {
                    iced::futures::stream::pending().boxed()
                } else {
                    iced::futures::stream::select_all(batteries).boxed()
                };

                let device_property_events = if device_properties.is_empty() {
                    iced::futures::stream::pending().boxed()
                } else {
                    iced::futures::stream::select_all(device_properties).boxed()
                };

                Box::pin(stream_select!(
                    interface_changed,
                    powered,
                    discovering,
                    rfkill,
                    battery_events,
                    device_property_events,
                ))
            }
            _ => interface_changed,
        };

        Ok(combined)
    }

    async fn start_listening(state: State, output: &mut Sender<ServiceEvent<Self>>) -> State {
        match state {
            State::Init => match zbus::Connection::system().await {
                Ok(conn) => {
                    let data = BluetoothService::initialize_data(&conn).await;

                    match data {
                        Ok(data) => {
                            info!("Bluetooth service initialized");

                            let _ = output
                                .send(ServiceEvent::Init(BluetoothService {
                                    data,
                                    conn: conn.clone(),
                                }))
                                .await;

                            State::Active(conn)
                        }
                        Err(err) => {
                            error!("Failed to initialize bluetooth service: {err}");

                            State::Error
                        }
                    }
                }
                Err(err) => {
                    error!("Failed to connect to system bus: {err}");

                    State::Error
                }
            },
            State::Active(conn) => {
                info!("Listening for bluetooth events");

                match BluetoothService::events(&conn).await {
                    Ok(mut events) => {
                        while events.next().await.is_some() {
                            if let Ok(data) = BluetoothService::initialize_data(&conn).await {
                                let _ = output.send(ServiceEvent::Update(data)).await;
                            }
                        }

                        State::Active(conn)
                    }
                    Err(err) => {
                        error!("Failed to listen for bluetooth events: {err}");
                        State::Error
                    }
                }
            }
            State::Error => {
                error!("Bluetooth service error, retrying in 5 seconds");

                sleep(Duration::from_secs(5)).await;

                State::Init
            }
        }
    }

    async fn toggle_power(conn: &zbus::Connection, power: bool) -> anyhow::Result<()> {
        let bluetooth = BluetoothDbus::new(conn).await?;

        bluetooth.set_powered(power).await?;

        Ok(())
    }

    fn execute_operation(&self, operation: BluetoothCommand) -> Task<ServiceEvent<Self>> {
        let conn = self.conn.clone();

        Task::perform(
            async move {
                if let Ok(bluetooth) = BluetoothDbus::new(&conn).await {
                    match &operation {
                        // Handled by the caller via toggle_power.
                        BluetoothCommand::Toggle => {}
                        BluetoothCommand::StartDiscovery => {
                            if let Err(e) = bluetooth.start_discovery().await {
                                warn!("Failed to start discovery: {e}");
                            }

                            // Auto-stop after 15 seconds
                            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                            if let Err(e) = bluetooth.stop_discovery().await {
                                warn!("Failed to stop discovery: {e}");
                            }
                        }
                        BluetoothCommand::PairDevice(device_path) => {
                            debug!("Pairing device: {:?}", device_path);
                            if let Err(e) = bluetooth.pair_device(device_path).await {
                                warn!("Failed to pair device: {e}");
                            }
                        }
                        BluetoothCommand::ConnectDevice(device_path) => {
                            debug!("Connecting device: {:?}", device_path);
                            if let Err(e) = bluetooth.connect_device(device_path).await {
                                warn!("Failed to connect device: {e}");
                            }
                        }
                        BluetoothCommand::DisconnectDevice(device_path) => {
                            debug!("Disconnecting device: {:?}", device_path);
                            if let Err(e) = bluetooth.disconnect_device(device_path).await {
                                warn!("Failed to disconnect device: {e}");
                            }
                        }
                        BluetoothCommand::RemoveDevice(device_path) => {
                            debug!("Removing device: {:?}", device_path);
                            if let Err(e) = bluetooth.remove_device(device_path).await {
                                warn!("Failed to remove device: {e}");
                            }
                        }
                    }
                }
                BluetoothService::initialize_data(&conn)
                    .await
                    .unwrap_or_else(|_| BluetoothData {
                        state: BluetoothState::Unavailable,
                        devices: vec![],
                        discovering: false,
                    })
            },
            ServiceEvent::Update,
        )
    }
}

impl ReadOnlyService for BluetoothService {
    type UpdateEvent = BluetoothData;
    type Error = ();

    fn update(&mut self, event: Self::UpdateEvent) {
        self.data = event;
    }

    impl_service_subscription!(BluetoothService, 100);
}

impl Service for BluetoothService {
    type Command = BluetoothCommand;

    fn command(&mut self, command: Self::Command) -> Task<ServiceEvent<Self>> {
        match command {
            BluetoothCommand::Toggle => {
                let conn = self.conn.clone();

                if self.data.state == BluetoothState::Unavailable {
                    Task::none()
                } else {
                    let mut data = self.data.clone();

                    Task::perform(
                        async move {
                            let powered = data.state == BluetoothState::Active;
                            debug!("Toggling bluetooth power to: {}", !powered);
                            let res = BluetoothService::toggle_power(&conn, !powered).await;

                            if res.is_ok() {
                                data.state = if powered {
                                    BluetoothState::Inactive
                                } else {
                                    BluetoothState::Active
                                }
                            }

                            data
                        },
                        ServiceEvent::Update,
                    )
                }
            }
            BluetoothCommand::StartDiscovery => {
                self.execute_operation(BluetoothCommand::StartDiscovery)
            }
            BluetoothCommand::PairDevice(device_path) => {
                self.execute_operation(BluetoothCommand::PairDevice(device_path))
            }
            BluetoothCommand::ConnectDevice(device_path) => {
                self.execute_operation(BluetoothCommand::ConnectDevice(device_path))
            }
            BluetoothCommand::DisconnectDevice(device_path) => {
                self.execute_operation(BluetoothCommand::DisconnectDevice(device_path))
            }
            BluetoothCommand::RemoveDevice(device_path) => {
                self.execute_operation(BluetoothCommand::RemoveDevice(device_path))
            }
        }
    }
}
