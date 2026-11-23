use std::{
    fs,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

use procfs::{ExplicitSystemInfo, FromRead, FromReadSI, KernelStats, Meminfo};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

use crate::idle::Idle;
use crate::metrics::{Sampler, cpu, disk_usage, memory, percent, temperature};
use crate::notifications::Center;
use crate::protocol::EVENTS;

fn events() -> Vec<Value> {
    EVENTS.with(|e| e.take())
}
fn cmd(center: &mut Center, value: Value) -> Vec<Value> {
    center
        .handle(serde_json::from_value(value).unwrap())
        .unwrap();
    events()
}
fn received(id: u32, token: &str, over: Value, restore: bool) -> Value {
    let mut row = json!({"id":"","app":"Test","summary":"Summary","body":"<plain text>","icon":"image://live","urgency":1,"transient":false,"timeout":-1,"actions":[{"identifier":"open","text":"Open"}],"closed":false});
    row.as_object_mut()
        .unwrap()
        .extend(over.as_object().unwrap().clone());
    json!({"kind":"received","native":id,"token":token,"time":100,"row":row,"restore":restore})
}
fn center(t: &TempDir) -> Center {
    let mut c = Center::default();
    c.restore(t.path().join("new"), &t.path().join("old"), false, false)
        .unwrap();
    c
}
#[test]
fn sampler_only_reads_visible_and_resets_baseline() {
    let now = Instant::now();
    let mut s = Sampler::new(now);
    let reads = std::cell::Cell::new(0);
    let disk = || {
        reads.set(reads.get() + 1);
        Some([5.; 3])
    };
    let sample = |prev: &mut Option<(u64, u64)>, d: [f64; 3]| {
        let fresh = prev.is_none();
        *prev = Some((100, 80));
        json!({"fresh":fresh,"disk":d})
    };
    assert!(s.tick(now, || panic!(), |_, _| panic!()).is_none());
    assert_eq!(s.wait(now), Duration::from_secs(3600));
    s.activate(true, 1, now);
    let e = s.tick(now, disk, sample).unwrap();
    assert_eq!(e["fresh"], true);
    assert_eq!(e["generation"], 1);
    s.activate(true, 7, now + Duration::from_secs(1));
    assert_eq!(s.wait(now), Duration::from_millis(2500));
    assert!(
        s.tick(
            now + Duration::from_millis(2499),
            || panic!(),
            |_, _| panic!()
        )
        .is_none()
    );
    for n in 1..12 {
        let e = s
            .tick(now + Duration::from_millis(2500 * n), || panic!(), sample)
            .unwrap();
        assert_eq!(e["fresh"], false);
        assert_eq!(e["generation"], 7);
        assert_eq!(e["disk"], json!([5., 5., 5.]));
    }
    assert_eq!(
        s.tick(now + Duration::from_secs(30), || None, sample)
            .unwrap()["disk"],
        json!([-1., -1., -1.])
    );
    s.activate(false, 2, now);
    assert!(
        s.tick(now + Duration::from_secs(100), || panic!(), |_, _| panic!())
            .is_none()
    );
    s.activate(true, 3, now + Duration::from_secs(101));
    let e = s
        .tick(now + Duration::from_secs(101), disk, sample)
        .unwrap();
    assert_eq!(e["fresh"], true);
    assert_eq!(e["generation"], 3);
    assert_eq!(reads.get(), 2);
}
#[test]
fn telemetry_cpu_counters_and_deltas() {
    let parse = |s: &str| {
        KernelStats::from_read(
            format!("{s}\nctxt 0\nbtime 0\nprocesses 0\n").as_bytes(),
            &ExplicitSystemInfo {
                boot_time_secs: 0,
                ticks_per_second: 100,
                page_size: 4096,
                is_little_endian: true,
            },
        )
    };
    let stats = parse("cpu 100 20 30 400 50 6 7 8 20 5\ncpu0 1 2 3 4").unwrap();
    let before = cpu(&stats.total);
    assert_eq!(before, Some((621, 450)));
    let after = cpu(&parse("cpu 140 20 40 440 60 6 7 8 60 5").unwrap().total);
    assert_eq!(percent(before, after), Some(50.));
    assert_eq!(percent(Some((10, 5)), Some((13, 6))), Some(67.));
    assert_eq!(percent(Some((10, 5)), Some((20, 15))), Some(0.));
    assert_eq!(percent(Some((10, 5)), Some((20, 5))), Some(100.));
    for (p, n) in [
        (None, after),
        (before, before),
        (after, before),
        (before, None),
        (Some((10, 5)), Some((20, 16))),
        (Some((10, 5)), Some((20, 4))),
    ] {
        assert!(percent(p, n).is_none())
    }
    for s in [
        "",
        "cpu0 1 2 3 4",
        "cpu 1 2 3",
        "cpu 1 -2 3 4",
        "cpu 1 NaN 3 4",
        "cpu 1 2 3 4x",
        "cpu malformed\ncpu 1 2 3 4",
    ] {
        assert!(parse(s).is_err())
    }
    assert_eq!(cpu(&parse("cpu 1 2 3 4\r\n").unwrap().total), Some((10, 4)));
    // procfs validates the complete stat snapshot, including per-CPU rows.
    assert!(parse("cpu 1 2 3 4\ncpu0 malformed").is_err());
    let mut c = stats.total;
    c.guest = Some(u64::MAX);
    c.guest_nice = Some(u64::MAX);
    assert_eq!(cpu(&c), before);
    c.user = u64::MAX;
    assert!(cpu(&c).is_none());
    c.user = 0;
    c.idle = u64::MAX;
    c.iowait = Some(1);
    assert!(cpu(&c).is_none());
}
#[test]
fn telemetry_memory_from_library_data() {
    let text = "MemTotal: 2097152 kB\nMemFree: 0 kB\nMemAvailable: 524288 kB\n\
        Buffers: 0 kB\nCached: 0 kB\nSwapCached: 0 kB\nActive: 0 kB\nInactive: 0 kB\n\
        SwapTotal: 0 kB\nSwapFree: 0 kB\nDirty: 0 kB\nWriteback: 0 kB\nMapped: 0 kB\n\
        Slab: 0 kB\nCommitted_AS: 0 kB\nVmallocTotal: 0 kB\nVmallocUsed: 0 kB\nVmallocChunk: 0 kB\n";
    let mut m = Meminfo::from_read(text.as_bytes()).unwrap();
    assert_eq!(m.mem_total, 2 << 30);
    assert_eq!(memory(&m), Some([75., 1.5, 2.]));
    m.mem_available = Some(0);
    m.mem_free = m.mem_total;
    assert_eq!(memory(&m), Some([100., 2., 2.]));
    m.mem_available = Some(m.mem_total + 1);
    assert_eq!(memory(&m), Some([0., 0., 2.]));
    m = Meminfo::from_read(text.replace("MemAvailable: 524288 kB\n", "").as_bytes()).unwrap();
    assert_eq!(m.mem_available, None);
    m.mem_free = 1 << 28;
    m.buffers = 1 << 28;
    m.cached = 1 << 29;
    m.s_reclaimable = Some(1 << 30);
    assert_eq!(memory(&m), Some([50., 1., 2.]));
    m.mem_free = u64::MAX;
    assert!(memory(&m).is_none());
    m.mem_free = 0;
    m.buffers = u64::MAX;
    assert!(memory(&m).is_none());
    // An available value takes precedence even if the unused fallback would overflow.
    m.mem_available = Some(1 << 30);
    assert_eq!(memory(&m), Some([50., 1., 2.]));
    m.mem_total = 0;
    assert!(memory(&m).is_none());
    for s in [
        String::new(),
        "MemTotal: 100 kB".into(),
        text.replace("MemTotal: 2097152 kB\n", ""),
        text.replace("MemFree: 0 kB\n", ""),
        text.replace("2097152 kB", "nonsense kB"),
        text.replace("2097152 kB", "-1 kB"),
        text.replace("2097152 kB", "2097152 invalid"),
    ] {
        assert!(Meminfo::from_read(s.as_bytes()).is_err())
    }
}
#[test]
fn telemetry_disk_and_temperature_boundaries() {
    assert_eq!(disk_usage(4, 3, 2, 1 << 30), Some([34., 1., 4.]));
    assert_eq!(disk_usage(100, 100, 95, 4096).unwrap()[0], 0.);
    assert!(disk_usage(0, 0, 0, 4096).is_none());
    assert!(disk_usage(100, 101, 95, 4096).is_none());
    assert!(disk_usage(100, 100, 0, 4096).is_none());
    assert!(disk_usage(100, 50, 40, 0).is_none());
    assert_eq!(temperature("45678\n"), Some(45.678));
    assert_eq!(temperature(" 52.5 "), Some(52.5));
    assert_eq!(temperature("1000"), Some(1000.));
    assert_eq!(temperature("1001"), Some(1.001));
    for s in ["", "0", "-1", "NaN", "45C", "40000\n50000"] {
        assert!(temperature(s).is_none())
    }
}
#[test]
fn idle_transaction_restore_exit_and_cleanup() {
    let t = tempdir().unwrap();
    let path = t.path().join("nested/screen-timeout.json");
    let exe = t.path().join("fake-watcher");
    fs::write(&exe, "#!/bin/sh\nexec sleep 60\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let restore = || {
        Idle::restore(
            path.clone(),
            exe.clone().into(),
            "/test/system'ctl with spaces".into(),
        )
    };
    let mut idle = restore();
    assert!(!idle.set(0).unwrap());
    assert!(!path.exists());
    for m in [10, 30, 60, 120, 180, 360] {
        let old = idle.child.as_ref().map(|w| w.0.id());
        assert!(idle.set(m).unwrap());
        assert_eq!(idle.minutes, m);
        let pid = idle.child.as_ref().unwrap().0.id();
        assert!(!idle.set(m).unwrap());
        assert_eq!(idle.child.as_ref().unwrap().0.id(), pid);
        if let Some(old) = old {
            assert_eq!(unsafe { libc::kill(old as i32, 0) }, -1)
        }
        assert_eq!(
            idle.command(m).get_args().collect::<Vec<_>>(),
            [
                "-w",
                "-C",
                "/dev/null",
                "timeout",
                &(u32::from(m) * 60).to_string(),
                "exec '/test/system'\\''ctl with spaces' suspend"
            ]
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
            json!({"version":1,"minutes":m})
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        )
    }
    let pid = idle.child.as_ref().unwrap().0.id();
    for m in [1, 5, 20, 61, 361] {
        assert!(idle.set(m).is_err())
    }
    idle.watcher = t.path().join("missing").into();
    assert!(idle.set(30).is_err());
    assert_eq!(idle.child.as_ref().unwrap().0.id(), pid);
    let saved = fs::read(&path).unwrap();
    idle.watcher = "false".into();
    assert!(idle.set(30).is_err());
    assert_eq!(idle.child.as_mut().unwrap().0.try_wait().unwrap(), None);
    assert_eq!(idle.child.as_ref().unwrap().0.id(), pid);
    assert_eq!(idle.minutes, 360);
    assert_eq!(fs::read(&path).unwrap(), saved);
    idle.watcher = exe.clone().into();
    idle.path = path.join("bad");
    for m in [30, 0] {
        assert!(idle.set(m).is_err());
        assert_eq!(idle.child.as_ref().unwrap().0.id(), pid);
        assert_eq!(idle.minutes, 360)
    }
    idle.path = path.clone();
    drop(idle);
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    let mut idle = restore();
    assert_eq!(idle.minutes, 360);
    idle.child.as_mut().unwrap().0.kill().unwrap();
    idle.child.as_mut().unwrap().0.wait().unwrap();
    assert!(idle.exited().unwrap());
    assert_eq!(idle.minutes, 0);
    assert_eq!(restore().minutes, 360);
    assert!(idle.set(0).unwrap());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["minutes"],
        0
    );
    assert!(!idle.set(0).unwrap());
    assert!(idle.child.is_none());
    assert_eq!(restore().minutes, 0);
}
#[test]
fn idle_malformed_oversize_boundary_and_failed_restore() {
    let t = tempdir().unwrap();
    let p = t.path().join("state");
    let restore = |exe: &str| Idle::restore(p.clone(), exe.into(), "ignored".into());
    let valid = "{\"version\":1,\"minutes\":10}";
    for s in [
        "broken",
        "{}",
        "{\"version\":2,\"minutes\":10}",
        "{\"version\":1,\"minutes\":11}",
        &format!("{valid:1025}"),
    ] {
        fs::write(&p, s).unwrap();
        assert_eq!(restore("true").minutes, 0);
        assert_eq!(fs::read_to_string(&p).unwrap(), s)
    }
    let s = format!("{valid:1024}");
    fs::write(&p, &s).unwrap();
    let exe = t.path().join("watcher");
    fs::write(&exe, "#!/bin/sh\nexec sleep 60\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(restore(exe.to_str().unwrap()).minutes, 10);
    assert_eq!(restore("/missing/dashboard-test").minutes, 0);
    assert_eq!(fs::read_to_string(p).unwrap(), s)
}
#[test]
fn notification_lifetime_hover_and_close() {
    let t = tempdir().unwrap();
    let mut c = center(&t);
    let start = Instant::now();
    cmd(&mut c, received(1, "one", json!({}), false));
    let id = c.history[0]["id"].clone();
    let now = Instant::now();
    assert!(
        (start + Duration::from_secs(8)..=now + Duration::from_secs(8))
            .contains(&c.live[0].due.unwrap())
    );
    cmd(
        &mut c,
        json!({"kind":"hover","id":id,"token":"one","value":true}),
    );
    assert!(c.live[0].due.is_none());
    cmd(&mut c, received(1, "two", json!({"timeout":600}), false));
    assert_eq!(c.history[0]["id"], id);
    assert!(c.live[0].hover);
    assert_eq!(c.history.len(), 1);
    for token in ["one", "two"] {
        let e = cmd(
            &mut c,
            json!({"kind":"action","id":id,"token":token,"identifier":"open"}),
        );
        assert_eq!(e.len(), usize::from(token == "two"));
        if token == "two" {
            assert_eq!(e[0]["operation"], "invoke")
        }
    }
    assert!(
        cmd(
            &mut c,
            json!({"kind":"action","id":id,"token":"two","identifier":"missing"})
        )
        .is_empty()
    );
    cmd(&mut c, json!({"kind":"dismiss","id":id,"token":"one"}));
    assert_eq!(c.history.len(), 1);
    let now = Instant::now();
    cmd(
        &mut c,
        json!({"kind":"hover","id":id,"token":"two","value":false}),
    );
    let due = c.live[0].due.unwrap();
    assert!(due >= now + Duration::from_millis(600));
    c.tick(due - Duration::from_nanos(1), Duration::MAX)
        .unwrap();
    assert!(events().is_empty());
    c.tick(due, Duration::MAX).unwrap();
    assert_eq!(events()[0]["token"], "two");
    cmd(&mut c, json!({"kind":"resolved","token":"one","reason":3}));
    assert!(!c.live.is_empty());
    cmd(&mut c, json!({"kind":"resolved","token":"two","reason":1}));
    assert!(c.popups.is_empty());
    assert_eq!(c.history[0]["closed"], true);
    assert_eq!(c.history[0]["actions"], json!([]));
    assert_eq!(c.history[0]["icon"], "");
    assert!(
        cmd(
            &mut c,
            json!({"kind":"action","id":id,"token":"two","identifier":"open"})
        )
        .is_empty()
    );
    cmd(&mut c, json!({"kind":"dismiss","id":id,"token":""}));
    assert!(c.history.is_empty());
    for (i, row) in [json!({"timeout":0}), json!({"urgency":2,"timeout":50})]
        .into_iter()
        .enumerate()
    {
        cmd(&mut c, received(2, "three", row, false));
        assert!(c.live[0].due.is_none());
        cmd(
            &mut c,
            json!({"kind":"resolved","token":"three","reason":2+i}),
        );
        assert!(c.history.is_empty())
    }
}
#[test]
fn pending_seed_survives_interruption_but_never_overrides_user_preference() {
    for user in [false, true] {
        let t = tempdir().unwrap();
        let legacy = t.path().join("old");
        fs::write(&legacy, br#"{"version":1,"dnd":true,"history":[]}"#).unwrap();
        let mut c = center(&t);
        let path = c.path.clone();
        assert_eq!(events()[0]["value"], true);
        cmd(&mut c, json!({"kind":"dnd","value":false}));
        let saved = || serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved()["pendingSeed"], true);
        drop(c);
        fs::remove_file(&legacy).unwrap();
        let mut c = Center::default();
        c.restore(path.clone(), &legacy, false, user).unwrap();
        if user {
            assert!(events().is_empty());
            assert!(!c.dnd);
            c.publish().unwrap();
            events();
        } else {
            assert_eq!(events(), vec![json!({"event":"seed","value":true})]);
            cmd(&mut c, json!({"kind":"seedAck","value":true}));
        }
        assert!(saved().get("pendingSeed").is_none());
        assert_eq!(saved()["dnd"], !user);
        let _c = center(&t);
        assert!(events().is_empty());
    }
}
#[test]
fn migration_dnd_admission_and_reattachment() {
    let t = tempdir().unwrap();
    let legacy = t.path().join("old");
    let row = json!({"id":"legacy","app":"app","summary":"s","body":"b","icon":"image://old","timeout":-8,"urgency":5});
    fs::write(&legacy,json!({"version":1,"dnd":true,"history":[null,{},row,row,{"id":"t","app":"a","summary":"s","body":"b","transient":true}]}).to_string()).unwrap();
    let mut explicit = Center::default();
    explicit
        .restore(t.path().join("explicit"), &legacy, false, true)
        .unwrap();
    assert!(!explicit.dnd);
    assert!(events().is_empty());
    let mut c = center(&t);
    assert_eq!(events()[0]["event"], "seed");
    assert!(c.dnd);
    assert_eq!(c.history.len(), 1);
    assert_eq!(c.history[0]["timeout"], -1);
    assert_eq!(c.history[0]["urgency"], 1);
    assert_eq!(c.history[0]["icon"], "");
    let e = cmd(
        &mut c,
        received(4, "quiet", json!({"transient":true}), false),
    );
    assert_eq!(e[0]["operation"], "expire");
    assert!(c.popups.is_empty());
    cmd(&mut c, json!({"kind":"seedAck","value":true}));
    cmd(&mut c, json!({"kind":"dnd","value":false}));
    cmd(&mut c, received(2, "first", json!({}), false));
    cmd(&mut c, received(1, "next", json!({}), false));
    cmd(
        &mut c,
        received(2, "replacement", json!({"summary":"updated"}), false),
    );
    let ids = c
        .history
        .iter()
        .map(|v| v["id"].clone())
        .collect::<Vec<_>>();
    let mut c = center(&t);
    assert!(!c.dnd);
    assert!(events().is_empty());
    for (id, token, over) in [
        (1, "reattach1", json!({})),
        (2, "reattach2", json!({"summary":"updated"})),
    ] {
        cmd(&mut c, received(id, token, over, true));
    }
    assert_eq!(c.live.len(), 2);
    assert!(c.popups.is_empty());
    assert_eq!(
        c.history
            .iter()
            .map(|v| v["id"].clone())
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(c.history[0]["actions"][0]["identifier"], "open");
    cmd(
        &mut c,
        received(3, "transient", json!({"transient":true}), false),
    );
    let e = cmd(&mut c, json!({"kind":"dnd","value":true}));
    assert_eq!(e[0]["token"], "transient");
    assert!(c.popups.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&c.path).unwrap()).unwrap()["dnd"],
        true
    );
    let e = cmd(&mut c, json!({"kind":"clear"}));
    assert_eq!(e.iter().filter(|v| v["operation"] == "dismiss").count(), 2);
    let mut c = center(&t);
    assert!(c.history.is_empty());
    assert_eq!(
        cmd(&mut c, received(1, "stale", json!({}), true))[0]["operation"],
        "expire"
    );
    fs::write(&c.path, "broken").unwrap();
    assert!(center(&t).history.is_empty())
}
#[test]
fn bounded_order_pruning_and_hover_eviction() {
    let t = tempdir().unwrap();
    let mut c = center(&t);
    cmd(&mut c, received(1, "1", json!({}), false));
    let id = c.history[0]["id"].clone();
    cmd(
        &mut c,
        json!({"kind":"hover","id":id,"token":"1","value":true}),
    );
    for i in 2..=201 {
        let e = cmd(&mut c, received(i, &i.to_string(), json!({}), false));
        if i == 6 {
            assert!(!c.live[0].hover);
            assert!(c.live[0].due.is_some())
        }
        if i == 201 {
            assert_eq!(e[0]["operation"], "expire");
            assert_eq!(e[0]["token"], "1")
        }
    }
    assert_eq!(c.history.len(), 200);
    assert_eq!(c.popups.len(), 5);
    assert_eq!(c.live.len(), 200);
    assert_eq!(c.history[0]["token"], "201");
    assert_eq!(c.popups[4]["token"], "197");
    assert_eq!(center(&t).history.len(), 200)
}
