use super::*;

use gloo_events::EventListener;
use send_wrapper::SendWrapper;
use web_sys::{
    DedicatedWorkerGlobalScope, Event, ServiceWorkerGlobalScope, SharedWorkerGlobalScope, Window,
};

pub struct GlobalEventListenerWrapper<T> {
    _event_listener: SendWrapper<EventListener>,
    receiver: flume::Receiver<T>,
}

pub type GlobalEventListenerHandler<T> = Box<dyn Fn(&Event) -> Option<T> + 'static>;

impl<T> GlobalEventListenerWrapper<T>
where
    T: 'static,
{
    pub fn new(event_name: &str, handler: GlobalEventListenerHandler<T>) -> EyreResult<Self> {
        let global_obj = js_sys::global();

        let (sender, receiver) = flume::bounded(1);

        let event_listener = if global_obj.is_instance_of::<Window>() {
            let obj: Window = global_obj.unchecked_into();
            EventListener::new(&obj, event_name.to_static_str(), move |e: &Event| {
                if let Some(v) = handler(e) {
                    sender.try_send(v).unwrap();
                }
            })
        } else if global_obj.is_instance_of::<DedicatedWorkerGlobalScope>() {
            let obj: DedicatedWorkerGlobalScope = global_obj.unchecked_into();
            EventListener::new(&obj, event_name.to_static_str(), move |e: &Event| {
                if let Some(v) = handler(e) {
                    sender.try_send(v).unwrap();
                }
            })
        } else if global_obj.is_instance_of::<SharedWorkerGlobalScope>() {
            let obj: SharedWorkerGlobalScope = global_obj.unchecked_into();
            EventListener::new(&obj, event_name.to_static_str(), move |e: &Event| {
                if let Some(v) = handler(e) {
                    sender.try_send(v).unwrap();
                }
            })
        } else if global_obj.is_instance_of::<ServiceWorkerGlobalScope>() {
            let obj: ServiceWorkerGlobalScope = global_obj.unchecked_into();
            EventListener::new(&obj, event_name.to_static_str(), move |e: &Event| {
                if let Some(v) = handler(e) {
                    sender.try_send(v).unwrap();
                }
            })
        } else {
            return Err(eyre!("Invalid global object type"));
        };

        Ok(Self {
            _event_listener: SendWrapper::new(event_listener),
            receiver,
        })
    }

    pub fn receiver(&self) -> flume::Receiver<T> {
        self.receiver.clone()
    }
}
