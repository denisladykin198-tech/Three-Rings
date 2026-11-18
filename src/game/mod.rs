//! Контракт «игра как плагин».
//! Ни room.rs, ни net.rs не знают правил: они спрашивают
//! «сколько мест», «чей ход», «что можно этому месту», «что ему видно».
//!
//! Новая игра = один модуль + две строки в registry()/create().

pub mod rings;
pub mod ttt;

pub type Seat = usize;
pub const SPECTATOR: Seat = usize::MAX;

/// Разобранная команда `act <id> k=v k=v`.
pub struct Cmd<'a> {
    pub id: &'a str,
    pub args: Vec<(&'a str, &'a str)>,
}

impl<'a> Cmd<'a> {
    pub fn get(&self, k: &str) -> Option<&'a str> {
        self.args.iter().find(|(a, _)| *a == k).map(|(_, v)| *v)
    }
    pub fn num(&self, k: &str) -> Option<i64> {
        self.get(k)?.parse().ok()
    }
    pub fn flag(&self, k: &str) -> bool {
        matches!(self.get(k), Some("1") | Some("true") | Some("yes"))
    }
}

/// Разрешённое действие. Клиент рисует кнопки ровно отсюда.
pub struct Action { pub id: &'static str, pub label: &'static str, pub arg: &'static str }

impl Action {
    pub fn new(id: &'static str, label: &'static str) -> Self {
        Self { id, label, arg: "" }
    }
    pub fn with(id: &'static str, label: &'static str, arg: &'static str) -> Self {
        Self { id, label, arg }
    }
    pub fn arg(id: &'static str, label: &'static str, arg: &'static str) -> Self {
        Self { id, label, arg }
    }
    
}

pub enum Step {
    Reject(String),
    Ok { events: Vec<String> },
}

impl Step {
    pub fn ok(events: Vec<String>) -> Self {
        Step::Ok { events }
    }
    pub fn one(e: String) -> Self {
        Step::Ok { events: vec![e] }
    }
    pub fn quiet() -> Self {
        Step::Ok { events: vec![] }
    }
    pub fn no(msg: impl Into<String>) -> Self {
        Step::Reject(msg.into())
    }
}

pub trait Game: Send + 'static {
    fn id(&self) -> &'static str;
    /// сколько мест занято конфигурацией партии (включая ботов)
    fn seats(&self) -> usize;
    /// минимум живых людей для старта
    fn min_humans(&self) -> usize { 1 }
    fn started(&self) -> bool;
    fn over(&self) -> bool;
    /// чей ход; None — ход не персональный
    fn turn(&self) -> Option<Seat>;
    /// место управляется сервером (бот)
    fn is_bot(&self, seat: Seat) -> bool;

    fn actions(&self, seat: Seat) -> Vec<Action>;
    fn apply(&mut self, seat: Seat, cmd: &Cmd) -> Step;

    /// Персональное состояние. Скрытое (колоды, чужие карты) вырезается ЗДЕСЬ.
    fn view(&self, seat: Seat) -> String;

    /// Старт партии (когда набрались люди). Пустые события = ничего не изменилось.
    fn begin(&mut self) -> Vec<String> { vec![] }

    /// Таймеры: ход бота, автопас, конец аукциона. now_ms — монотонные мс.
    fn tick(&mut self, _now_ms: u64) -> Option<Vec<String>> { None }

    /// Игрок отвалился: по умолчанию место становится ботом, если игра умеет.
    fn on_leave(&mut self, _seat: Seat) -> Vec<String> { vec![] }
    fn on_join(&mut self, _seat: Seat) -> Vec<String> { vec![] }

    /// Имя игрока (для отображения). По умолчанию ничего не делает.
    fn set_name(&mut self, _seat: Seat, _name: &str) {}
    /// Партия сброшена/пересоздана в той же комнате.
    /// Клиент обязан пересобрать поле и снять экран победы.
    fn reset(&mut self) -> Vec<String> { vec![] }
}

pub struct Meta {
    pub id: &'static str,
    pub title: &'static str,
    pub max_seats: usize,
    pub hint: &'static str,
}

// ------------------------------------------------------------------ реестр
pub fn registry() -> Vec<Meta> {
    vec![
        Meta { id: "rings", title: "Три кольца", max_seats: 4, hint: "монополия · 48 клеток · 3 орбиты" },
        Meta { id: "ttt",   title: "Крестики-нолики", max_seats: 2, hint: "тест универсальности каркаса" },
    ]
}
pub fn create(id: &str, cfg: &[(&str, &str)]) -> Option<Box<dyn Game>> {
    match id.trim() {
        "rings" => Some(Box::new(rings::Rings::new(cfg))),
        "ttt"   => Some(Box::new(ttt::Ttt::new())),
        _ => None,
    }
}

pub fn max_seats(id: &str) -> usize {
    registry()
        .iter()
        .find(|m| m.id == id.trim())
        .map(|m| m.max_seats)
        .unwrap_or(2)
}

// ------------------------------------------------------------------ JSON без serde
/// Минимальный писатель JSON. Ровно то, что нужно протоколу.
pub struct J(String);

impl J {
    pub fn obj() -> Self { J("{".into()) }
    pub fn arr() -> Self { J("[".into()) }

    /// Запятая нужна, если контейнер уже не пуст.
    /// Смотрим ровно на открывающую скобку — никаких пробелов-разделителей.
    fn sep(&mut self) {
        if self.0.len() > 1 {
            self.0.push(',');
        }
    }
    pub fn key(&mut self, k: &str) -> &mut Self {
        self.sep();
        self.0.push_str(&quote(k));
        self.0.push(':');
        self
    }
    pub fn raw(&mut self, v: &str) -> &mut Self { self.0.push_str(v); self }
    pub fn n(&mut self, k: &str, v: i64) -> &mut Self { self.key(k).raw(&v.to_string()) }
    pub fn u(&mut self, k: &str, v: usize) -> &mut Self { self.n(k, v as i64) }
    pub fn b(&mut self, k: &str, v: bool) -> &mut Self {
        self.key(k).raw(if v { "true" } else { "false" })
    }
    pub fn s(&mut self, k: &str, v: &str) -> &mut Self { self.key(k).raw(&quote(v)) }
    /// null или число — для «владельца нет»
    pub fn opt(&mut self, k: &str, v: Option<usize>) -> &mut Self {
        match v {
            Some(x) => self.u(k, x),
            None => self.key(k).raw("null"),
        }
    }
    pub fn list(&mut self, k: &str, items: &[String]) -> &mut Self {
        self.key(k).raw(&format!("[{}]", items.join(",")))
    }
    pub fn nums(&mut self, k: &str, items: &[i64]) -> &mut Self {
        let v: Vec<String> = items.iter().map(|x| x.to_string()).collect();
        self.list(k, &v)
    }
    pub fn push(&mut self, item: &str) -> &mut Self { self.sep(); self.raw(item) }
    pub fn end(mut self) -> String {
        let open = self.0.as_bytes()[0];
        self.0.push(if open == b'[' { ']' } else { '}' });
        self.0
    }
}

pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Событие журнала: `{"t":"dice","p":0,"a":3,"b":4}`
pub fn ev(t: &str, build: impl FnOnce(&mut J)) -> String {
    let mut j = J::obj();
    j.s("t", t);
    build(&mut j);
    j.end()
}

// ------------------------------------------------------------------ ГПСЧ
/// splitmix64 + hash-chain: состояние после броска публикуется вместе с ним.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    pub fn die(&mut self) -> u8 { 1 + self.below(6) as u8 }
    pub fn shuffle<T>(&mut self, a: &mut [T]) {
        for i in (1..a.len()).rev() {
            let j = self.below(i + 1);
            a.swap(i, j);
        }
    }
}