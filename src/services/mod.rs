//! Backend services: D-Bus, compositor IPC, sysfs and Wayland connections
//! behind a common subscription pattern.
//!
//! Each service runs a `State` machine (`Init` → `Active`, `Error` → retry
//! after 5 s) inside its subscription, emitting [`ServiceEvent`]s: `Init`
//! once with the first usable value, then `Update`s. Modules keep the `Init`
//! value and apply updates to it — usually via [`ServiceEvent::apply`].
//! Services that accept commands additionally implement [`Service`], whose
//! `command` returns a `Task` with the follow-up events.

use iced::{Subscription, Task};

pub mod audio;
pub mod bluetooth;
pub mod brightness;
pub mod compositor;
pub mod idle_inhibitor;
pub mod logind;
pub mod mpris;
pub mod network;
pub mod notifications;
pub mod privacy;
pub mod rfkill;
mod throttle;
pub mod tray;
pub mod upower;
pub mod xdg_icons;

/// Lifecycle of a service as seen by the module holding it: `Init` once when
/// the backend is first reachable (carrying the initial value), `Update` for
/// each change, `Error` when the backend is lost. The subscription keeps
/// retrying after an `Error`; modules normally just ignore it.
#[derive(Debug, Clone)]
pub enum ServiceEvent<S: ReadOnlyService> {
    Init(S),
    Update(S::UpdateEvent),
    Error(S::Error),
}

/// Outcome of applying a [`ServiceEvent`] to a module's service slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// Init service stored.
    Init,
    /// Update event forwarded to the live service.
    Updated,
    /// Update with no service stored yet, or an Error event — nothing to do.
    Ignored,
}

impl<S: ReadOnlyService> ServiceEvent<S> {
    /// Standard module-side handling: store Init, forward Update to the
    /// stored service, drop Error. Returns the outcome so callers can refresh
    /// derived state. Modules that must inspect the update event before it is
    /// consumed keep their manual match.
    pub fn apply(self, service: &mut Option<S>) -> Applied {
        match self {
            ServiceEvent::Init(s) => {
                *service = Some(s);
                Applied::Init
            }
            ServiceEvent::Update(event) => {
                if let Some(s) = service.as_mut() {
                    s.update(event);
                    Applied::Updated
                } else {
                    Applied::Ignored
                }
            }
            ServiceEvent::Error(_) => Applied::Ignored,
        }
    }
}

/// A [`ReadOnlyService`] that also accepts commands. `command` performs the
/// backend call and returns a `Task` yielding the follow-up [`ServiceEvent`]s.
pub trait Service: ReadOnlyService {
    type Command;

    fn command(&mut self, command: Self::Command) -> Task<ServiceEvent<Self>>;
}

/// A service the UI only observes. `subscribe` drives a state machine that
/// emits [`ServiceEvent`]s; `update` applies one event to the stored value.
pub trait ReadOnlyService: Sized {
    type UpdateEvent;
    type Error: Clone;

    fn update(&mut self, event: Self::UpdateEvent);

    fn subscribe() -> Subscription<ServiceEvent<Self>>;
}

/// Implements `ReadOnlyService::subscribe` with the standard subscription
/// loop: a channel that repeatedly calls `start_listening` on the service's
/// `State` machine. Requires `State` and `start_listening` to be in scope in
/// the invoking module. (`NetworkService`, `CompositorService` and
/// `LogindService` hand-roll theirs.)
///
/// `send` errors are ignored, so events are dropped — not queued — when the
/// receiving module can't keep up; the capacity is a burst buffer, sized per
/// service by its caller.
macro_rules! impl_service_subscription {
    ($ty:ty, $capacity:expr) => {
        fn subscribe() -> iced::Subscription<super::ServiceEvent<$ty>> {
            iced::Subscription::run_with(std::any::TypeId::of::<$ty>(), |_| {
                iced::stream::channel($capacity, async |mut output| {
                    let mut state = State::Init;
                    loop {
                        state = <$ty>::start_listening(state, &mut output).await;
                    }
                })
            })
        }
    };
}
pub(crate) use impl_service_subscription;
