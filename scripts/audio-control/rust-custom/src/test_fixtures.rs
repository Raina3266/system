use crate::{
    AppResult,
    audio::{Action, Backend, State},
    devices::Metadata,
    model::{AudioKind, Snapshot},
};
use serde_json::{Value, json};

pub const CARD: &str = "alsa_card.test";
pub const SPEAKER: &str = "[Out] Speaker";
pub const HEADPHONES: &str = "[Out] Headphones";

pub fn node(id: u32, name: &str, kind: &str, device: Option<u32>) -> Value {
    json!({"id": id, "serial": (id * 10).to_string(), "name": name,
        "description": "Alder Lake PCH-P High Definition Audio Controller Analog Stereo",
        "kind": kind, "cardId": device.map(|_| 7), "cardSerial": if device.is_some() {"70"} else {""},
        "profileDevice": device, "routes": [], "routeName": "wrong-card-wide-route"})
}

pub fn native_routes(kind: &str) -> Value {
    let dump = dump();
    json!(
        dump[0]["info"]["params"]["EnumRoute"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|route| route["direction"] == kind)
            .map(|route| json!({
                "index": route["index"], "name": route["name"], "description": route["description"],
                "available": route["available"] != "no"
            }))
            .collect::<Vec<_>>()
    )
}

pub fn snapshot() -> Snapshot {
    let mut speaker = node(10, "speaker-sink", "output", Some(3));
    speaker["routes"] = native_routes("Output");
    let mut mic = node(12, "microphone", "input", Some(5));
    mic["routes"] = native_routes("Input");
    let mut virtual_node = node(20, "virtual", "output", None);
    virtual_node["description"] = "Virtual Output".into();
    serde_json::from_value(json!({
        "cards": [{"id":7,"serial":"70","name":CARD,"activeProfile":2,
            "profiles":[{"index":0},{"index":1},{"index":2},{"index":3}]}],
        "nodes": [speaker, mic, virtual_node],
        "defaultOutput":{"id":10,"serial":"100"}, "defaultInput":{"id":12,"serial":"120"}
    }))
    .unwrap()
}

pub fn dump() -> Value {
    let profile = |index, name, sinks, sources, priority| {
        json!({
            "index": index, "name":name,"description":"IDENTICAL DESCRIPTION, NEVER AN IDENTITY",
            "available":"yes","priority":priority,
            "classes":[2,["Audio/Source",2,"card.profile.devices",sources],
                ["Audio/Sink",4,"card.profile.devices",sinks]]
        })
    };
    let route = |index, name, label, direction, profiles, devices, available| {
        json!({
            "index":index,"name":name,"description":label,"direction":direction,
            "profiles":profiles,"devices":devices,"available":available
        })
    };
    json!([{"id":7,"type":"PipeWire:Interface:Device","info":{
        "props":{"object.serial":70,"device.name":CARD,"device.description":"DO NOT USE RAW INVENTORY"},
        "params":{
            "EnumProfile":[
                {"index":0,"name":"off","priority":0,"available":"yes","classes":[0]},
                profile(1,"headphones",json!([0,1,2,6]),json!([4,5]),10300),
                profile(2,"speakers",json!([0,1,2,3]),json!([4,5]),10200),
                {"index":3,"name":"pro-audio","priority":1,"available":"unknown",
                    "classes":[2,["Audio/Source",1,"card.profile.devices",[11]],
                        ["Audio/Sink",1,"card.profile.devices",[10]]]}
            ],
            "EnumRoute":[
                route(0,"[Out] HDMI3","HDMI / DisplayPort 3","Output",json!([2,1]),json!([0]),"no"),
                route(1,"[Out] HDMI2","HDMI / DisplayPort 2","Output",json!([2,1]),json!([1]),"yes"),
                route(2,"[Out] HDMI1","HDMI / DisplayPort 1","Output",json!([2,1]),json!([2]),"yes"),
                route(3,SPEAKER,"Speaker","Output",json!([2]),json!([3]),"unknown"),
                route(4,"[In] Mic2","Microphone jack","Input",json!([2,1]),json!([4]),"no"),
                route(5,"[In] Mic1","Internal microphone","Input",json!([2,1]),json!([5]),"unknown"),
                route(6,HEADPHONES,"Headphones","Output",json!([1]),json!([6]),"yes")
            ],
            "Route":[{"index":3,"device":3,"profile":2},{"index":5,"device":5,"profile":2}]
        }
    }}])
}

pub fn mixed_alsa_bluez() -> (Snapshot, Value) {
    let mut native = snapshot();
    native.cards.push(
        serde_json::from_value(json!({
            "id":8,"serial":"80","name":"bluez_card.test","activeProfile":1,
            "profiles":[{"index":0},{"index":1},{"index":2}]
        }))
        .unwrap(),
    );
    for (id, name, kind, device, route) in [
        (30, "bluez_output.test", "output", 1, 10),
        (31, "bluez_input.test", "input", 0, 11),
    ] {
        let mut endpoint = node(id, name, kind, Some(device));
        endpoint["cardId"] = 8.into();
        endpoint["cardSerial"] = "80".into();
        endpoint["description"] = "Bluetooth Headset".into();
        endpoint["routes"] = json!([{"index":route,"name":format!("{kind}-headset"),
            "description":"Headset","available":true}]);
        native.nodes.push(serde_json::from_value(endpoint).unwrap());
    }
    let mut raw = dump();
    raw.as_array_mut().unwrap().push(json!({
        "id":8,"type":"PipeWire:Interface:Device","info":{
            "props":{"object.serial":80,"device.name":"bluez_card.test"},
            "params":{
                // BlueZ omits empty classes and does not prefix active classes with a count.
                "EnumProfile":[
                    {"index":0,"name":"off","available":"yes","priority":0},
                    {"index":1,"name":"headset-head-unit","available":"yes","priority":20,
                        "classes":[["Audio/Source",1,"card.profile.devices",[0]],
                            ["Audio/Sink",1,"card.profile.devices",[1]]]},
                    {"index":2,"name":"audio-gateway","available":"yes","priority":256}
                ],
                "EnumRoute":[
                    {"index":10,"name":"output-headset","description":"Headset",
                        "direction":"Output","available":"yes","profiles":[1]},
                    {"index":11,"name":"input-headset","description":"Headset",
                        "direction":"Input","available":"yes","profiles":[1]}
                ],
                "Route":[{"index":10,"device":1},{"index":11,"device":0}]
            }
        }
    }));
    (native, raw)
}

pub fn state(snapshot: Snapshot, dump: &Value) -> State {
    snapshot.validate().unwrap();
    let metadata = Metadata::from_dump(&snapshot, AudioKind::Output, dump).unwrap();
    State { snapshot, metadata }
}

pub fn initial() -> State {
    state(snapshot(), &dump())
}

pub fn offline() -> Snapshot {
    serde_json::from_value(json!({"cards":[],"nodes":[{
        "id":20,"serial":"200","name":"virtual","description":"Virtual Output",
        "kind":"output","cardId":null,"cardSerial":"","profileDevice":null,"routes":[],"routeName":""
    }],"defaultOutput":null,"defaultInput":null})).unwrap()
}

pub struct Fake {
    pub snapshot: Snapshot,
    pub dump: Value,
    pub events: Vec<Action>,
    pub delay: usize,
    pub never_appears: bool,
    pub reject_profile: bool,
    pub reject_route: bool,
    pub reject_default: bool,
    pub ignore_route: bool,
    pub external_change: bool,
    pub reject_restore: bool,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            snapshot: snapshot(),
            dump: dump(),
            events: Vec::new(),
            delay: 0,
            never_appears: false,
            reject_profile: false,
            reject_route: false,
            reject_default: false,
            ignore_route: false,
            external_change: false,
            reject_restore: false,
        }
    }
}

impl Backend for Fake {
    fn observe(&mut self) -> AppResult<State> {
        let profile = self.snapshot.cards.first().map(|card| card.active_profile);
        if profile == Some(1) && self.external_change {
            self.snapshot.cards[0].active_profile = 3;
        } else if let Some(profile @ (1 | 2)) = profile {
            let name = if profile == 1 {
                "headphone-sink"
            } else {
                "speaker-sink"
            };
            if !self.snapshot.nodes.iter().any(|node| node.name == name) {
                if self.delay > 0 {
                    self.delay -= 1;
                } else if profile == 2 || !self.never_appears {
                    let mut value = node(
                        if profile == 1 { 11 } else { 10 },
                        name,
                        "output",
                        Some(if profile == 1 { 6 } else { 3 }),
                    );
                    value["routes"] = native_routes("Output");
                    if profile == 2 {
                        value["serial"] = "101".into();
                    }
                    self.snapshot.nodes.push(serde_json::from_value(value)?);
                }
            }
        }
        Ok(state(self.snapshot.clone(), &self.dump))
    }

    fn action(&mut self, action: Action) -> AppResult<()> {
        self.events.push(action.clone());
        match action {
            Action::Profile { card, profile } => {
                assert_eq!(card, self.snapshot.cards[0].identity());
                if (profile == 1 && self.reject_profile) || (profile == 2 && self.reject_restore) {
                    return Err("Profile rejected".into());
                }
                self.snapshot.cards[0].active_profile = profile;
                self.snapshot
                    .nodes
                    .retain(|node| node.name != "speaker-sink" && node.name != "headphone-sink");
                if self
                    .snapshot
                    .default_output
                    .as_ref()
                    .is_some_and(|identity| self.snapshot.node(identity).is_err())
                {
                    self.snapshot.default_output = None;
                }
            }
            Action::Route { node, route } => {
                if self.reject_route {
                    return Err("Route rejected".into());
                }
                let endpoint = self.snapshot.node(&node)?.clone();
                if !self.ignore_route {
                    let profile = self.snapshot.cards[0].active_profile;
                    let routes = self.dump[0]["info"]["params"]["Route"]
                        .as_array_mut()
                        .unwrap();
                    routes.retain(|route| {
                        route["device"].as_u64() != endpoint.profile_device.map(u64::from)
                    });
                    routes.push(
                        json!({"index":route,"device":endpoint.profile_device,"profile":profile}),
                    );
                }
            }
            Action::Default { node } => {
                let endpoint = self.snapshot.node(&node)?;
                if self.reject_default && endpoint.name == "headphone-sink" {
                    return Err("Default rejected".into());
                }
                match endpoint.kind {
                    AudioKind::Output => self.snapshot.default_output = Some(node),
                    AudioKind::Input => self.snapshot.default_input = Some(node),
                }
            }
        }
        Ok(())
    }
    fn pause(&mut self) {}
}

pub mod resident {
    use crate::{
        AppResult,
        model::{AudioKind, NativeSnapshot, Snapshot},
        policy::panel::Native,
    };
    use serde_json::Value;

    pub fn native(snapshot: Snapshot) -> NativeSnapshot {
        let mut objects = Vec::new();
        for card in snapshot.cards {
            let mut value = serde_json::to_value(card).unwrap();
            value["key"] = format!("card:{}", value["id"]).into();
            value["type"] = "card".into();
            objects.push(value);
        }
        for node in snapshot.nodes {
            let mut value = serde_json::to_value(node).unwrap();
            value["key"] =
                format!("node:{}:{}", value["id"], value["serial"].as_str().unwrap()).into();
            value["type"] = "node".into();
            value["volume"] = 0.4.into();
            value["muted"] = false.into();
            value["nick"] = "".into();
            objects.push(value);
        }
        let output = snapshot
            .default_output
            .map(|id| format!("node:{}:{}", id.id, id.serial))
            .unwrap_or_default();
        let input = snapshot
            .default_input
            .map(|id| format!("node:{}:{}", id.id, id.serial))
            .unwrap_or_default();
        for object in &mut objects {
            if object["type"] == "node" {
                object["isDefault"] = (object["key"] == output || object["key"] == input).into();
            }
        }
        NativeSnapshot {
            objects,
            output_epoch: format!("epoch:{output}"),
            input_epoch: format!("epoch:{input}"),
            default_output: output,
            default_input: input,
            ..Default::default()
        }
    }
    pub fn simple(kind: AudioKind) -> NativeSnapshot {
        let mut snapshot = super::offline();
        snapshot.nodes[0].kind = kind;
        let id = Some(snapshot.nodes[0].identity());
        match kind {
            AudioKind::Output => snapshot.default_output = id,
            AudioKind::Input => snapshot.default_input = id,
        }
        native(snapshot)
    }
    pub type Hook = Box<dyn FnMut(&mut Fake, &str)>;
    pub struct Fake {
        pub state: NativeSnapshot,
        pub actions: Vec<(String, String, Value)>,
        pub polls: usize,
        pub(crate) ignore: String,
        pub fail: String,
        pub hook: Option<Hook>,
    }
    impl Fake {
        pub fn new(state: NativeSnapshot) -> Self {
            Self {
                state,
                actions: Vec::new(),
                polls: 0,
                ignore: String::new(),
                fail: String::new(),
                hook: None,
            }
        }
        pub(crate) fn object_mut(&mut self, key: &str) -> &mut Value {
            self.state
                .objects
                .iter_mut()
                .find(|object| object["key"] == key)
                .unwrap()
        }
        fn fire(&mut self, operation: &str) {
            if let Some(mut hook) = self.hook.take() {
                hook(self, operation);
                self.hook = Some(hook);
            }
        }
    }
    impl Native for Fake {
        fn snapshot(&mut self) -> AppResult<NativeSnapshot> {
            self.fire("snapshot");
            Ok(self.state.clone())
        }
        fn execute(
            &mut self,
            operation: &str,
            target: &str,
            args: Value,
            guards: Vec<Value>,
            _: bool,
        ) -> AppResult<NativeSnapshot> {
            let root = serde_json::to_value(&self.state)?;
            for guard in guards {
                let actual = if guard["key"].is_null() {
                    &root
                } else {
                    self.state.object(guard["key"].as_str().unwrap())?
                };
                if guard["expect"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .any(|(key, value)| actual[key] != *value)
                {
                    return Err("Native object changed before the guarded operation".into());
                }
            }
            self.actions
                .push((operation.into(), target.into(), args.clone()));
            if self.fail == operation || self.fail == target {
                return Err(format!("Native {operation} rejected for {target}").into());
            }
            if self.ignore != operation {
                match operation {
                    "route" => {
                        let node = self.object_mut(target);
                        let name = node["routes"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|route| route["index"] == args["route"])
                            .unwrap()["name"]
                            .clone();
                        node["routeName"] = name;
                    }
                    "default" => {
                        let kind = self.state.object(target)?["kind"].clone();
                        for object in &mut self.state.objects {
                            if object["type"] == "node" && object["kind"] == kind {
                                object["isDefault"] = (object["key"] == target).into();
                            }
                        }
                        if self.state.object(target)?["kind"] == "output" {
                            self.state.default_output = target.into();
                            self.state.output_epoch.push('x');
                        } else {
                            self.state.default_input = target.into();
                            self.state.input_epoch.push('x');
                        }
                    }
                    "follow" => self.object_mut(target)["targetSerial"] = (-1).into(),
                    "volume" => self.object_mut(target)["volume"] = args["volume"].clone(),
                    "mute" => self.object_mut(target)["muted"] = args["muted"].clone(),
                    "trust" => self.object_mut(target)["trusted"] = args["trusted"].clone(),
                    "power" => self.object_mut(target)["powered"] = args["powered"].clone(),
                    "connect" => self.object_mut(target)["connected"] = true.into(),
                    _ => {}
                }
            }
            self.fire(operation);
            Ok(self.state.clone())
        }
        fn pause(&mut self) {
            self.polls += 1;
            self.fire("pause");
        }
    }
}
