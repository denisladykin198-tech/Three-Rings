mod game;
mod net;
mod room;

use std::collections::HashMap;
use std::io::{self, BufReader, Read};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use game::{Cmd, Step};
use net::Client;
use room::Room;

const INDEX: &str = include_str!("../static/index.html");
type Rooms = HashMap<String, Room>;
type Shared = Arc<Mutex<Rooms>>;
type Me = Option<(String, usize)>;

// ------------------------------------------------------------------ утилиты
fn rnd() -> u64 {
    static C: AtomicU64 = AtomicU64::new(0);
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
    let mut z = t ^ C.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn rnd_code(n: usize) -> String {
    const A: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut x = rnd();
    (0..n).map(|_| { let c = A[(x & 31) as usize]; x >>= 5; c as char }).collect()
}

// Используем монотонный таймер
static START_INSTANT: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
fn now_ms() -> u64 {
    START_INSTANT.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Значение name=... + хвост слов без '=' (имена с пробелами).
fn name_from(rest: &[&str]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut on = false;
    for t in rest {
        if let Some(v) = t.strip_prefix("name=") { on = true; parts.push(v); }
        else if t.contains('=') { on = false; }   // началась другая пара — имя кончилось
        else if on { parts.push(t); }
    }
    parts.join(" ").trim().chars().take(16).collect()
}

/// Хост не должен быть пустым местом или ботом.
fn rehost(r: &mut Room) {
    let ok = r.host.is_some_and(|h| r.players[h].is_some() && !r.game.is_bot(h));
    if ok { return; }
    r.host = (0..r.players.len()).find(|&i| r.players[i].is_some() && !r.game.is_bot(i));
}

// ------------------------------------------------------------------ выход
fn leave(rooms: &mut Rooms, me: &mut Me, c: &Arc<Client>) {
    let Some((code, slot)) = me.take() else { return };
    let Some(r) = rooms.get_mut(&code) else { return };
    if !r.players[slot].as_ref().is_some_and(|p| Arc::ptr_eq(p, c)) { return; }
    r.players[slot] = None;
    let mut evs = r.game.on_leave(slot);
    evs.push(game::ev("leave", |j| { j.u("seat", slot); }));
    if r.players.iter().all(|p| p.is_none()) {
        rooms.remove(&code);
    } else {
        rehost(r);
        r.publish(evs);
    }
}

// ------------------------------------------------------------------ протокол
fn dispatch(rooms: &mut Rooms, me: &mut Me, c: &Arc<Client>, line: &str) {
    let mut it = line.split_whitespace();
    match it.next().unwrap_or("") {
        "games" => {
            let mut arr = game::J::arr();
            for m in game::registry() {
                let mut o = game::J::obj();
                o.s("id", m.id).s("title", m.title).u("max", m.max_seats).s("hint", m.hint);
                arr.push(&o.end());
            }
            c.send(&format!("games {}", arr.end()));
        }

        "create" => {
            leave(rooms, me, c);
            let Some(game_id) = it.next() else { c.send("error Не указана игра"); return; };
            let rest: Vec<&str> = it.collect();
            let pairs: Vec<(&str, &str)> =
                rest.iter().map(|kv| kv.split_once('=').unwrap_or((kv, ""))).collect();
            let name = name_from(&rest);

            let Some(g) = game::create(game_id, &pairs)
                else { c.send("error Неизвестная игра"); return; };

            let code = loop {
                let k = rnd_code(4);
                if !rooms.contains_key(&k) { break k; }
            };
            let n = g.seats().max(1);
            let room = Room {
                code: code.clone(),
                game_id: game_id.trim().to_string(),
                game: g,
                players: (0..n).map(|_| None).collect(),
                tokens: (0..n).map(|_| rnd_code(8)).collect(),
                names: (0..n).map(|_| String::new()).collect(),
                seq: 0,
                host: Some(0),
            };
            rooms.insert(code.clone(), room);
            let r = rooms.get_mut(&code).unwrap();
            r.players[0] = Some(c.clone());
            let mut evs = r.game.on_join(0);
            if !name.is_empty() {
                r.names[0] = name.clone();
                r.game.set_name(0, &name);
            }
            *me = Some((code.clone(), 0));
            c.send(&format!("joined {} 0 {}", code, r.tokens[0]));
            evs.push(game::ev("create", |j| { j.s("game", &r.game_id).u("seats", n); }));
            r.publish(evs);
        }

"room" => {
    let Some((code, _)) = me.clone() else { c.send("error Ты не в комнате"); return; };
    let Some(r) = rooms.get(&code) else { return; };
    c.send(&format!("room {}", r.summary_json()));
}

        "join" => {
            let code = it.next().unwrap_or("").to_uppercase();
            let rest: Vec<&str> = it.collect();

            // Ключ места — ТОЛЬКО первый позиционный аргумент и только без '='.
            let tok: &str = rest.first().copied().filter(|s| !s.contains('=')).unwrap_or("");

            let name = name_from(&rest);

            if code.is_empty() { c.send("error Пустой код комнаты"); return; }
            
            // Если мы уже в этой комнате, просто пересылаем текущее состояние (реконнект)
            if let Some((cur_code, slot)) = me.clone() {
                if cur_code == code {
                    if let Some(r) = rooms.get_mut(&code) {
                        // Проверяем, что этот сокет действительно владеет этим слотом
                        if r.players[slot].as_ref().is_some_and(|p| Arc::ptr_eq(p, c)) {
                            c.send(&format!("joined {} {} {}", code, slot, r.tokens[slot]));
                            r.broadcast_views();
                            return;
                        }
                    }
                    // Если сокет не совпадает, выходим из старой комнаты
                    *me = None;
                }
            }
            
            leave(rooms, me, c);

            let Some(r) = rooms.get_mut(&code) else { c.send("error Комната не найдена"); return; };

            let slot = if tok.is_empty() {
                // Вход без токена: ищем свободное место, только если игра еще не началась
                if r.game.started() {
                    c.send("error Партия уже идёт — нужен ключ места");
                    return;
                }
                r.players.iter().position(|p| p.is_none())
            } else {
                // Вход по токену
                match r.tokens.iter().position(|t| *t == tok) {
                    Some(s) => {
                        // Если место занято другим сокетом, убиваем старый сокет
                        if let Some(old) = r.players[s].take() {
                            if !Arc::ptr_eq(&old, c) { old.kill(); }
                        }
                        Some(s)
                    }
                    None => { c.send("error Ключ не подходит"); return; }
                }
            };

            let Some(slot) = slot else { c.send("error В комнате нет свободных мест"); return; };

            r.players[slot] = Some(c.clone());
            
            // Обновляем имя, если оно передано
            if !name.is_empty() {
                r.names[slot] = name.clone();
                r.game.set_name(slot, &name);
            }
            
            let mut evs = r.game.on_join(slot);
            evs.push(game::ev("join", |j| { j.u("seat", slot); }));
            
            let token = r.tokens[slot].clone();
            *me = Some((code.clone(), slot));
            
            c.send(&format!("joined {} {} {}", code, slot, token));
            r.publish(evs);
        }

        "start" => {
            let Some((code, _)) = me.clone() else { c.send("error Ты не в комнате"); return; };
            let Some(r) = rooms.get_mut(&code) else { return };
            let seat = r.players.iter()
                .position(|p| p.as_ref().is_some_and(|cl| Arc::ptr_eq(cl, c)));
            rehost(r);
            if r.host != seat || seat.is_none() {
                c.send("error Начать партию может только хост");
                return;
            }
            if r.game.started() { c.send("error Партия уже началась"); return; }
            let humans = r.players.iter().filter(|p| p.is_some()).count();
            if humans < r.game.min_humans() {
                c.send(&format!("error Нужно минимум {} чел.", r.game.min_humans()));
                return;
            }
            let evs = r.game.begin();
            r.publish(evs);
        }

"restart" => {
    let Some((code, _)) = me.clone() else { c.send("error Ты не в комнате"); return; };
    let Some(r) = rooms.get_mut(&code) else { return };
    let seat = r.players.iter().position(|p| p.as_ref().is_some_and(|cl| Arc::ptr_eq(cl, c)));
    rehost(r);
    if r.host != seat || seat.is_none() { c.send("error Пересоздать партию может только хост"); return; }
    if !r.game.over() && r.game.started() {
        c.send("error Партия ещё идёт"); return;
    }
    let mut evs = r.game.reset();
    // возвращаем живых людей на места (reset сделал всех ботами)
    for i in 0..r.players.len() {
        if r.players[i].is_some() {
            evs.extend(r.game.on_join(i));
            if !r.names[i].is_empty() { r.game.set_name(i, &r.names[i]); }
        }
    }
    r.publish(evs);
}

        "act" => {
            let Some((code, seat)) = me.clone() else { c.send("error Ты не в комнате"); return; };
            let act = it.next().unwrap_or("");
            let args: Vec<(&str, &str)> =
                it.map(|kv| kv.split_once('=').unwrap_or((kv, ""))).collect();
            let Some(r) = rooms.get_mut(&code) else { return; };
            if !r.players[seat].as_ref().is_some_and(|p| Arc::ptr_eq(p, c)) {
                c.send("error Место занято другим соединением");
                return;
            }
            if !r.game.actions(seat).iter().any(|a| a.id == act) {
                c.send("error Сейчас это недоступно");
                return;
            }
            let cmd = Cmd { id: act, args };
            match r.game.apply(seat, &cmd) {
                Step::Reject(e) => c.send(&format!("error {e}")),
                Step::Ok { events } => r.publish(events),
            }
        }

        "sync" => {
            let Some((code, _)) = me.clone() else { c.send("error Ты не в комнате"); return; };
            let Some(r) = rooms.get_mut(&code) else { return; };
            r.broadcast_views();
        }

        "leave" => {
            leave(rooms, me, c);
            c.send("left");
        }

        "rooms" => {
            let mut arr = game::J::arr();
            let mut v: Vec<&Room> = rooms.values().collect();
            v.sort_by(|a, b| a.code.cmp(&b.code));
            for r in v { arr.push(&r.summary_json()); }
            c.send(&format!("rooms {}", arr.end()));
        }

        _ => c.send("error Неизвестная команда"),
    }
}

// ------------------------------------------------------------------ тикер
fn spawn_ticker(state: Shared) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(200));
        let now = now_ms();
        let mut rooms = state.lock().unwrap();
        let mut dead: Vec<String> = Vec::new();
        for (code, r) in rooms.iter_mut() {
            // мёртвые сокеты освобождаем сразу, иначе комната висит вечно
            let mut gone: Vec<usize> = Vec::new();
            for (i, p) in r.players.iter().enumerate() {
                if let Some(cl) = p {
                    if !cl.alive() { gone.push(i); }
                }
            }
            let mut evs: Vec<String> = Vec::new();
            for i in gone {
                r.players[i] = None;
                evs.extend(r.game.on_leave(i));
                evs.push(game::ev("leave", |j| { j.u("seat", i); }));
            }
            if !evs.is_empty() { rehost(r); }
            if let Some(te) = r.game.tick(now) { evs.extend(te); }
            if !evs.is_empty() { r.publish(evs); }
            if r.players.iter().all(|p| p.is_none()) { dead.push(code.clone()); }
        }
        for code in dead { rooms.remove(&code); }
    });
}

// ------------------------------------------------------------------ сеть
fn serve(stream: TcpStream, state: Shared) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(45)))?;
    let mut rd = BufReader::new(stream.try_clone()?);
    let mut wr = stream.try_clone()?;

    let key = match net::read_req(&mut rd)? {
        Some(net::Req::Ws(k)) => k,
        Some(net::Req::Page(path)) => {
            let p = path.split('?').next().unwrap_or("/");
            match p {
                "/" | "/index.html" => net::write_html(&mut wr, INDEX)?,
                "/favicon.ico" | "/favicon.svg" => net::write_icon(&mut wr)?,
                "/rooms" => {
                    let rooms = state.lock().unwrap();
                    let mut arr = game::J::arr();
                    let mut v: Vec<&Room> = rooms.values().collect();
                    v.sort_by(|a, b| a.code.cmp(&b.code));
                    for r in v { arr.push(&r.summary_json()); }
                    net::write_json(&mut wr, &arr.end())?;
                }
                _ => net::write_404(&mut wr)?,
            }
            return Ok(());
        }
        None => return Ok(()),
    };

    net::write_101(&mut wr, &key)?;
    let client = Client::new(stream.try_clone()?);
    net::spawn_writer(stream.try_clone()?, client.clone());
    client.send("hello 2");
    let mut me: Me = None;
    let res = ws_loop(&mut rd, &state, &client, &mut me);
    client.kill();
    leave(&mut state.lock().unwrap(), &mut me, &client);
    res
}

fn ws_loop(rd: &mut impl Read, state: &Shared, c: &Arc<Client>, me: &mut Me) -> io::Result<()> {
    let mut frag: Vec<u8> = Vec::new();
    let mut frag_op = 0u8;
    while let Some((fin, op, p)) = net::read_frame(rd)? {
        match op {
            0x8 => break,
            0x9 => { c.pong(&p); continue; }
            0xA => continue,
            0x1 | 0x2 => {
                if !frag.is_empty() { break; }      // незакрытая фрагментация
                frag_op = op;
                frag.extend_from_slice(&p);
            }
            0x0 => {
                if frag.is_empty() { break; }
                frag.extend_from_slice(&p);
            }
            _ => break,
        }
        if frag.len() > net::MAX_MESSAGE { break; }
        if !fin { continue; }
        if frag_op != 0x1 { frag.clear(); continue; }   // бинарь не поддерживаем
        let Ok(text) = String::from_utf8(std::mem::take(&mut frag)) else { break };
        let mut rooms = state.lock().unwrap();
        dispatch(&mut rooms, me, c, text.trim());
    }
    Ok(())
}

fn lan_ip() -> Option<IpAddr> {
    for probe in ["8.8.8.8:80", "192.168.255.255:9", "10.255.255.255:9"] {
        let Ok(s) = UdpSocket::bind("0.0.0.0:0") else { continue; };
        if s.connect(probe).is_ok() {
            if let Ok(a) = s.local_addr() {
                if !a.ip().is_unspecified() { return Some(a.ip()); }
            }
        }
    }
    None
}

fn main() -> io::Result<()> {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);
    let state: Shared = Arc::new(Mutex::new(HashMap::new()));
    spawn_ticker(state.clone());
    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port)))
        .expect("не удалось занять порт");
    println!("\n  🎮 Три кольца · универсальный сервер\n");
    println!("  Ты:    http://localhost:{port}");
    if let Some(ip) = lan_ip() { println!("  Друг:  http://{ip}:{port}"); }
    println!("\n  Ctrl+C — стоп.\n");
    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                let st = state.clone();
                std::thread::spawn(move || {
                    if let Err(e) = serve(s, st) { eprintln!("[net] {e}"); }
                });
            }
            Err(e) => eprintln!("[accept] {e}"),
        }
    }
    Ok(())
}