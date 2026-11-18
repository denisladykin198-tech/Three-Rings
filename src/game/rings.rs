//! «Три кольца» — кастомная монополия. Плагин игры.
use super::{ev, Action, Cmd, Game, J, Rng, Seat, Step, SPECTATOR};
use std::time::{SystemTime, UNIX_EPOCH};

// ================================================================== ДАННЫЕ
const GROUPS: [(&str, &str, u32); 9] = [
    ("Доки", "#ff7a45", 50),
    ("Рынок", "#ffd23f", 50),
    ("Парки", "#3ef08c", 50),
    ("Промзона", "#2dd4bf", 50),
    ("Проспекты", "#38bdf8", 100),
    ("Культура", "#8b7bff", 100),
    ("Наука", "#e879f9", 100),
    ("Сервис", "#fb7185", 100),
    ("Башни Центра", "#f5c542", 200),
];

/// name, num, salary, k (клеток на сторону)
const RINGS: [(&str, &str, u32, usize); 3] = [
    ("Окраина", "I", 200, 5),
    ("Город", "II", 140, 3),
    ("Центр", "III", 80, 1),
];

pub const COLORS: [&str; 4] = ["#27e0ff", "#ff3d8b", "#ffb020", "#3ef08c"];
const BOT_NAMES: [&str; 4] = ["Неон", "Вектор", "Кварц", "Спутник"];
const START_CASH: i64 = 1500;
const JAIL_CELL: usize = 6; // кольцо I, idx 6
const JAIL_FEE: u32 = 50;
const TAX: u32 = 100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Start, Prop, Utility, Gate, Chance, Treasure, Tax, Parking, Jail, GoToJail, Market,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Start => "start", Kind::Prop => "prop", Kind::Utility => "utility",
            Kind::Gate => "gate", Kind::Chance => "chance", Kind::Treasure => "treasure",
            Kind::Tax => "tax", Kind::Parking => "parking", Kind::Jail => "jail",
            Kind::GoToJail => "gotojail", Kind::Market => "market",
        }
    }
}

const NG: u8 = 255; // «группы нет»

#[derive(Clone)]
pub struct Cell {
    pub kind: Kind,
    pub ring: u8,
    pub idx: u8,
    pub side: u8,
    pub group: u8,
    pub name: &'static str,
    pub price: u32,
    pub house: u32,
    pub rents: [u32; 6],
}

type Def = (Kind, u8, &'static str, u32);

const DEFS: [&[Def]; 3] = [
    &[
        (Kind::Start, NG, "Старт", 0),
        (Kind::Prop, 0, "Верфь", 60),
        (Kind::Prop, 0, "Маяк", 60),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 0, "Причал", 80),
        (Kind::Chance, NG, "Шанс", 0),
        (Kind::Jail, NG, "Изолятор", 0),
        (Kind::Prop, 1, "Рынок", 100),
        (Kind::Prop, 1, "Барахолка", 100),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 1, "Склады", 120),
        (Kind::Tax, NG, "Налог", 0),
        (Kind::Parking, NG, "Привал", 0),
        (Kind::Prop, 2, "Парк", 140),
        (Kind::Prop, 2, "Набережная", 140),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 2, "Бульвар", 160),
        (Kind::Chance, NG, "Шанс", 0),
        (Kind::GoToJail, NG, "Облава", 0),
        (Kind::Prop, 3, "Депо", 180),
        (Kind::Prop, 3, "Завод", 180),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 3, "ТЭЦ", 200),
        (Kind::Utility, NG, "Энергосеть", 150),
    ],
    &[
        (Kind::Start, NG, "Старт II", 0),
        (Kind::Prop, 4, "Проспект", 220),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 4, "Площадь", 240),
        (Kind::Treasure, NG, "Казна", 0),
        (Kind::Prop, 5, "Театр", 260),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 5, "Музей", 280),
        (Kind::Market, NG, "Биржа", 0),
        (Kind::Prop, 6, "Университет", 300),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 6, "Обсерватория", 320),
        (Kind::Chance, NG, "Шанс", 0),
        (Kind::Prop, 7, "Клиника", 340),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 7, "Аэропорт", 360),
    ],
    &[
        (Kind::Start, NG, "Старт III", 0),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 8, "Башня «Меридиан»", 400),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 8, "Башня «Зенит»", 450),
        (Kind::Gate, NG, "Ворота", 0),
        (Kind::Prop, 8, "Башня «Апогей»", 500),
        (Kind::Gate, NG, "Ворота", 0),
    ],
];

const CHANCE: [(&str, &str); 12] = [
    ("🚄", "Экспресс до Старта внешнего кольца. Получи ₽200."),
    ("🛗", "Лифт в Центр: отправляйся к башне «Зенит»."),
    ("🚨", "Облава! Отправляйся в Изолятор."),
    ("↩️", "Тупик: вернись на 3 клетки назад."),
    ("🌀", "Объездная: продвигайся до ближайших Ворот."),
    ("🎂", "День рождения! Каждый игрок дарит тебе ₽25."),
    ("🏎", "Штраф за скорость: заплати ₽50 в копилку Привала."),
    ("🔍", "Инспекция построек: заплати ₽25 за каждый уровень зданий."),
    ("🏦", "Банковский дивиденд: получи ₽50."),
    ("✈️", "Командировка: лети в Аэропорт (кольцо II)."),
    ("🗝", "Карта «Освобождение»: выход из Изолятора бесплатно."),
    ("⚡", "Ревизор зовёт: отправляйся к Энергосети."),
];

const TREASURE: [(&str, &str); 10] = [
    ("💰", "Банк ошибся в твою пользу: +₽100."),
    ("🧾", "Налоговый вычет: +₽50."),
    ("🏥", "Оплата лечения: −₽100 в копилку Привала."),
    ("🅿️", "Штраф за парковку: −₽50."),
    ("📜", "Наследство: +₽150."),
    ("🎖", "Городская премия: +₽75."),
    ("🚧", "Ремонт дорог: −₽20 за каждый уровень зданий."),
    ("🍕", "Угощаешь весь город: заплати каждому по ₽25."),
    ("🪙", "Возврат залога: +₽25."),
    ("🎰", "Джекпот! Забери всю копилку Привала."),
];

const MARKET: [(&str, i32); 6] = [
    ("Обвал рынка", -100), ("Коррекция", -50), ("Отскок", 50),
    ("Ралли", 100), ("Удачное IPO", 150), ("Бычий рынок", 200),
];

fn side_of(i: usize, k: usize) -> u8 {
    if i <= k { 0 } else if i <= 2 * k + 1 { 1 } else if i <= 3 * k + 2 { 2 } else { 3 }
}

fn base_rent(price: u32) -> u32 {
    let b = ((price as f64 * 0.12 / 5.0).round() as u32) * 5;
    b.max(5)
}

fn make_cells() -> Vec<Cell> {
    let mut out = Vec::new();
    for (r, defs) in DEFS.iter().enumerate() {
        let k = RINGS[r].3;
        for (i, &(kind, group, name, price)) in defs.iter().enumerate() {
            let b = base_rent(price);
            let house = if group == NG { 0 } else { GROUPS[group as usize].2 };
            out.push(Cell {
                kind, ring: r as u8, idx: i as u8, side: side_of(i, k), group, name, price, house,
                rents: [b, b * 5, b * 13, b * 24, b * 30, b * 36],
            });
        }
    }
    out
}

/// Статическая опись поля.
fn defs_json(cells: &[Cell]) -> String {
    let mut groups = J::arr();
    for (name, color, house) in GROUPS {
        let mut g = J::obj();
        g.s("name", name).s("color", color).n("house", house as i64);
        groups.push(&g.end());
    }
    let mut rings = J::arr();
    for (name, num, salary, k) in RINGS {
        let mut r = J::obj();
        r.s("name", name).s("num", num).n("salary", salary as i64).u("k", k);
        rings.push(&r.end());
    }
    let mut cs = J::arr();
    for (i, c) in cells.iter().enumerate() {
        let mut j = J::obj();
        j.u("i", i)
            .n("ring", c.ring as i64)
            .n("idx", c.idx as i64)
            .n("side", c.side as i64)
            .s("kind", c.kind.as_str())
            .s("name", c.name)
            .n("price", c.price as i64)
            .n("house", c.house as i64);
        if c.group == NG {
            j.key("group").raw("null");
        } else {
            j.n("group", c.group as i64);
        }
        if c.kind == Kind::Prop {
            j.nums("rents", &c.rents.iter().map(|&x| x as i64).collect::<Vec<_>>());
        }
        cs.push(&j.end());
    }
    let mut j = J::obj();
    j.key("groups").raw(&groups.end());
    j.key("rings").raw(&rings.end());
    j.key("cells").raw(&cs.end());
    j.end()
}

// ================================================================== МОДЕЛЬ
#[derive(Clone, Copy, PartialEq)]
enum Cred { Bank, Pot, Player(usize) }

impl Cred {
    fn code(self) -> i64 {
        match self { Cred::Bank => -1, Cred::Pot => -2, Cred::Player(i) => i as i64 }
    }
}

#[derive(Clone)]
struct Player {
    cash: i64, ring: u8, idx: u8, jail: u8, jail_free: bool, bankrupt: bool, bot: bool,
}

#[derive(Clone, Copy, Default)]
struct Estate { owner: Option<usize>, level: u8 }

#[derive(Clone, Copy, PartialEq)]
enum Diff { Careful, Balanced, Aggressive }

#[derive(Clone, Copy, PartialEq)]
enum Wait { Lobby, Run, Roll, Jail, Buy, Gate, Card, Market, Debt, End, Over }

impl Wait {
    fn as_str(self) -> &'static str {
        match self {
            Wait::Lobby => "lobby",
            Wait::Run => "run",
            Wait::Roll => "roll",
            Wait::Jail => "jail",
            Wait::Buy => "buy",
            Wait::Gate => "gate",
            Wait::Card => "card",
            Wait::Market => "market",
            Wait::Debt => "debt",
            Wait::End => "end",
            Wait::Over => "over",
        }
    }
}

#[derive(Clone)]
enum Task {
    Walk { steps: u8, back: bool },
    Teleport { cell: usize },
    Resolve,
    Buy { cell: usize },
    Gate,
    CardRun { deck: u8, card: u8 },
    Market { i: u8 },
    Pay { from: usize, amount: u32, to: Cred },
    Gain { p: usize, amount: u32 },
    PayEach { list: Vec<usize>, i: usize, amount: u32, incoming: bool },
    Pot { p: usize },
    Jail,
    Free,
    Unjail,
    AfterRoll,
    ForcedJailMove { steps: u8 }, // Новый таск для корректного выхода из тюрьмы
}

pub struct Rings {
    cells: Vec<Cell>,
    base: [usize; 3],
    len: [usize; 3],
    gate: [[usize; 4]; 3],
    group_cells: Vec<Vec<usize>>,
    defs: String,
    seats: usize,
    diff: Diff,
    players: Vec<Player>,
    names: Vec<String>,
    est: Vec<Estate>,
    decks: [Vec<u8>; 2],
    cur: usize,
    turn_no: u32,
    pot: u32,
    last_sum: u8,
    chain: u8,
    again: bool,
    started: bool,
    over: bool,
    winner: Option<usize>,
    wait: Wait,
    stack: Vec<Task>,
    events: Vec<String>,
    wait_cell: usize,
    wait_deck: u8,
    wait_card: u8,
    wait_market: u8,
    debt_amount: u32,
    debt_to: Cred,
    debt_who: usize,
    ans_b: bool,
    ans_i: i64,
    ans_ready: bool,
    rng: Rng,
    bot_at: u64,
    epoch: u64,          // NEW: идентификатор конкретной партии
}

impl Rings {
    pub fn new(cfg: &[(&str, &str)]) -> Self {
        let get = |k: &str| cfg.iter().find(|(a, _)| *a == k).map(|(_, v)| *v);

        let seats = get("seats")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(3)
            .clamp(1, 4);
        let diff = match get("bots").unwrap_or("balanced") {
            "careful" => Diff::Careful,
            "aggressive" => Diff::Aggressive,
            _ => Diff::Balanced,
        };
        let seed = get("seed").and_then(|v| v.parse::<u64>().ok()).unwrap_or_else(|| {
            SystemTime::now().duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64).unwrap_or(1)
        });

        let cells = make_cells();
        let mut base = [0usize; 3];
        let mut len = [0usize; 3];
        let mut acc = 0;
        for r in 0..3 {
            base[r] = acc;
            len[r] = 4 * RINGS[r].3 + 4;
            acc += len[r];
        }
        debug_assert_eq!(acc, cells.len(), "DEFS разошлись с RINGS");

        let mut gate = [[0usize; 4]; 3];
        let mut group_cells: Vec<Vec<usize>> = vec![Vec::new(); GROUPS.len()];
        for (i, c) in cells.iter().enumerate() {
            if c.kind == Kind::Gate { gate[c.ring as usize][c.side as usize] = i; }
            if c.kind == Kind::Prop { group_cells[c.group as usize].push(i); }
        }
        let defs = defs_json(&cells);

        let mut rng = Rng(seed);
        let mut chance: Vec<u8> = (0..CHANCE.len() as u8).collect();
        let mut treasure: Vec<u8> = (0..TREASURE.len() as u8).collect();
        rng.shuffle(&mut chance);
        rng.shuffle(&mut treasure);

        Self {
            cells, base, len, gate, group_cells, defs, seats, diff,
            players: (0..seats).map(|_| Player {
                cash: START_CASH, ring: 0, idx: 0, jail: 0,
                jail_free: false, bankrupt: false, bot: true,
            }).collect(),
            names: (0..seats).map(|i| BOT_NAMES[i % BOT_NAMES.len()].to_string()).collect(),
            est: vec![Estate::default(); acc],
            decks: [chance, treasure],
            cur: 0, turn_no: 0, pot: 0, last_sum: 7, chain: 0, again: false,
            started: false, over: false, winner: None,
            wait: Wait::Lobby, stack: Vec::new(), events: Vec::new(),
            wait_cell: 0, wait_deck: 0, wait_card: 0, wait_market: 0,
            debt_amount: 0, debt_to: Cred::Bank, debt_who: 0,
            ans_b: false, ans_i: 0, ans_ready: false, rng, bot_at: 0,
            epoch: seed,   
        }
    }

    // -------------------------------------------------------------- утилиты
    fn c(&self, i: usize) -> Cell { self.cells[i].clone() }
    fn emit(&mut self, s: String) { self.events.push(s); }
    fn cell_of(&self, p: usize) -> usize {
        let pl = &self.players[p];
        self.base[pl.ring as usize] + pl.idx as usize
    }
    fn set_wait(&mut self, w: Wait) { self.wait = w; self.bot_at = 0; }
    fn ask(&mut self, w: Wait, t: Task) {
        self.ans_ready = false;
        self.stack.push(t);
        self.set_wait(w);
    }
    fn answer(&mut self) {
        self.ans_ready = true;
        self.set_wait(Wait::Run);
        self.run();
    }
    fn actor(&self) -> usize {
        if self.wait == Wait::Debt { self.debt_who } else { self.cur }
    }
    fn others(&self, p: usize) -> Vec<usize> {
        (0..self.players.len()).filter(|&i| i != p && !self.players[i].bankrupt).collect()
    }
    fn full_group(&self, p: usize, g: u8) -> bool {
        g != NG && self.group_cells[g as usize].iter().all(|&i| self.est[i].owner == Some(p))
    }
    
    // Исправлено: убран хак __i, используется правильный индекс
    fn levels_sum(&self, p: usize) -> u32 {
        self.cells
            .iter()
            .enumerate()
            .filter(|(i, c)| c.kind == Kind::Prop && self.est[*i].owner == Some(p))
            .map(|(i, _)| self.est[i].level as u32)
            .sum()
    }

    fn net_worth(&self, p: usize) -> i64 {
        let mut w = self.players[p].cash;
        for (i, c) in self.cells.iter().enumerate() {
            if (c.kind == Kind::Prop || c.kind == Kind::Utility) && self.est[i].owner == Some(p) {
                w += c.price as i64 + self.est[i].level as i64 * c.house as i64;
            }
        }
        w
    }
    fn rent_of(&self, cell: usize) -> u32 {
        let c = self.c(cell);
        let e = self.est[cell];
        if c.kind == Kind::Utility { return 4 * self.last_sum as u32; }
        if e.level > 0 { return c.rents[e.level as usize]; }
        match e.owner {
            Some(o) if self.full_group(o, c.group) => c.rents[0] * 2,
            _ => c.rents[0],
        }
    }
    fn can_build(&self, p: usize, cell: usize) -> bool {
        let c = self.c(cell);
        let e = self.est[cell];
        if c.kind != Kind::Prop || e.owner != Some(p) || !self.full_group(p, c.group) { return false; }
        if e.level >= 5 || self.players[p].cash < c.house as i64 { return false; }
        let min = self.group_cells[c.group as usize].iter().map(|&i| self.est[i].level).min().unwrap_or(0);
        e.level == min
    }
    fn can_sell_b(&self, p: usize, cell: usize) -> bool {
        let c = self.c(cell);
        let e = self.est[cell];
        if c.kind != Kind::Prop || e.owner != Some(p) || e.level == 0 { return false; }
        let max = self.group_cells[c.group as usize].iter().map(|&i| self.est[i].level).max().unwrap_or(0);
        e.level == max
    }
    fn can_sell_p(&self, p: usize, cell: usize) -> bool {
        let c = self.c(cell);
        if self.est[cell].owner != Some(p) { return false; }
        if c.kind == Kind::Utility { return true; }
        if c.kind != Kind::Prop { return false; }
        self.group_cells[c.group as usize].iter().all(|&i| self.est[i].level == 0)
    }
    fn manage_ok(&self) -> bool { self.wait == Wait::Roll || self.wait == Wait::End }

    /// Кнопки управления активами — единый источник правды для actions() и act().
    fn manage_actions(&self, seat: usize, a: &mut Vec<Action>) {
        let manage = self.manage_ok();
        let debt = self.wait == Wait::Debt;
        if !manage && !debt { return; }
        let p = &self.players[seat];
        if (0..self.cells.len()).any(|i| self.can_build(seat, i)) {
            a.push(Action::with("build", "▲ Построить", "cell"));
        }
        if (0..self.cells.len()).any(|i| self.can_sell_b(seat, i)) {
            a.push(Action::with("sellb", "▼ Снести", "cell"));
        }
        if (0..self.cells.len()).any(|i| self.can_sell_p(seat, i)) {
            a.push(Action::with("sellp", "✕ Продать банку", "cell"));
        }
        if manage && p.cash >= 0 {
            a.push(Action::new("concede", "🏳 Сдаться"));
        }
    }

    // -------------------------------------------------------------- ВМ партии
    fn run(&mut self) {
        let mut guard = 0;
        while self.wait == Wait::Run && !self.over {
            guard += 1;
            if guard > 400 {
                self.stack.clear();
                self.set_wait(Wait::End);
                break;
            }
            if self.players[self.cur].bankrupt {
                self.stack.clear();
                self.next_turn();
                return;
            }
            let Some(t) = self.stack.pop() else {
                self.next_turn();
                return;
            };
            self.exec(t);
        }
        if self.over { self.set_wait(Wait::Over); }
    }

    fn exec(&mut self, t: Task) {
        match t {
            Task::Walk { steps, back } => self.do_walk(steps, back),
            Task::Teleport { cell } => {
                let p = self.cur;
                let c = self.c(cell);
                self.players[p].ring = c.ring;
                self.players[p].idx = c.idx;
                self.emit(ev("teleport", |j| { j.u("p", p).u("cell", cell); }));
            }
            Task::Resolve => self.do_resolve(),
            Task::Buy { cell } => self.do_buy(cell),
            Task::Gate => self.do_gate(),
            Task::CardRun { deck, card } => {
                if !self.players[self.cur].bot && !self.ans_ready {
                    self.wait_deck = deck;
                    self.wait_card = card;
                    self.ask(Wait::Card, Task::CardRun { deck, card });
                    return;
                }
                self.ans_ready = false;
                self.card_effect(deck, card);
            }
            Task::Market { i } => {
                if !self.players[self.cur].bot && !self.ans_ready {
                    self.wait_market = i;
                    self.ask(Wait::Market, Task::Market { i });
                    return;
                }
                self.ans_ready = false;
                let d = MARKET[i as usize].1;
                let p = self.cur;
                if d > 0 {
                    self.stack.push(Task::Gain { p, amount: d as u32 });
                } else {
                    self.stack.push(Task::Pay { from: p, amount: (-d) as u32, to: Cred::Bank });
                }
            }
            Task::Pay { from, amount, to } => self.do_pay(from, amount, to),
            Task::Gain { p, amount } => {
                self.players[p].cash += amount as i64;
                let cell = self.cell_of(p);
                self.emit(ev("gain", |j| { j.u("p", p).n("amount", amount as i64).u("cell", cell); }));
            }
            Task::PayEach { list, i, amount, incoming } => {
                let cont = Task::PayEach { list: list.clone(), i: i + 1, amount, incoming };
                self.stack.push(cont);
                if i >= list.len() || self.over { return; }
                let other = list[i];
                if self.players[other].bankrupt { return; }
                let (from, to) = if incoming {
                    (other, Cred::Player(self.cur))
                } else {
                    (self.cur, Cred::Player(other))
                };
                self.stack.push(Task::Pay { from, amount, to });
            }
            Task::Pot { p } => {
                if self.pot > 0 {
                    let v = self.pot;
                    self.pot = 0;
                    self.players[p].cash += v as i64;
                    let cell = self.cell_of(p);
                    self.emit(ev("pot", |j| { j.u("p", p).n("amount", v as i64).u("cell", cell); }));
                } else {
                    self.emit(ev("potempty", |j| { j.u("p", p); }));
                }
            }
            Task::Jail => {
                let p = self.cur;
                self.players[p].ring = 0;
                self.players[p].idx = JAIL_CELL as u8;
                self.players[p].jail = 1;
                self.again = false;
                self.chain = 0;
                self.emit(ev("jail", |j| { j.u("p", p); }));
            }
            Task::Free => {
                let p = self.cur;
                self.players[p].jail_free = true;
                self.emit(ev("free", |j| { j.u("p", p); }));
            }
            Task::Unjail => {
                let p = self.cur;
                self.players[p].jail = 0;
                self.emit(ev("jailout", |j| { j.u("p", p).s("how", "pay"); }));
                self.set_wait(Wait::Roll);
            }
            Task::AfterRoll => {
                if self.over || self.players[self.cur].bankrupt { return; }
                if self.again && self.players[self.cur].jail == 0 {
                    self.set_wait(Wait::Roll);
                } else {
                    self.set_wait(Wait::End);
                }
            }
            Task::ForcedJailMove { steps } => {
                let p = self.cur;
                if self.over || self.players[p].bankrupt { return; }
                self.players[p].jail = 0;
                self.again = false;
                self.emit(ev("jailout", |j| { j.u("p", p).s("how", "forced"); }));
                self.push_move(steps);
            }
        }
    }

    fn do_walk(&mut self, steps: u8, back: bool) {
        let p = self.cur;
        let ring = self.players[p].ring as usize;
        let len = self.len[ring];
        let base = self.base[ring];
        let mut idx = self.players[p].idx as usize;
        let mut path: Vec<i64> = Vec::new();
        let mut sal: Vec<i64> = Vec::new();
        let salary = RINGS[ring].2 as i64;
        for _ in 0..steps {
            idx = if back { (idx + len - 1) % len } else { (idx + 1) % len };
            path.push((base + idx) as i64);
            let s = if !back && idx == 0 { salary } else { 0 };
            if s > 0 { self.players[p].cash += s; }
            sal.push(s);
        }
        self.players[p].idx = idx as u8;
        self.emit(ev("walk", |j| { j.u("p", p).b("back", back).nums("path", &path).nums("sal", &sal); }));
    }

    fn do_resolve(&mut self) {
        let p = self.cur;
        if self.players[p].bankrupt || self.over { return; }
        let cell = self.cell_of(p);
        let c = self.c(cell);
        match c.kind {
            Kind::Prop | Kind::Utility => match self.est[cell].owner {
                None => self.stack.push(Task::Buy { cell }),
                Some(o) if o == p => {
                    self.emit(ev("own", |j| { j.u("p", p).u("cell", cell); }));
                }
                Some(o) => {
                    let rent = self.rent_of(cell);
                    self.emit(ev("rent", |j| { j.u("cell", cell).u("owner", o).n("amount", rent as i64); }));
                    self.stack.push(Task::Pay { from: p, amount: rent, to: Cred::Player(o) });
                }
            },
            Kind::Gate => self.stack.push(Task::Gate),
            Kind::Chance => self.draw(0),
            Kind::Treasure => self.draw(1),
            Kind::Tax => self.stack.push(Task::Pay { from: p, amount: TAX, to: Cred::Pot }),
            Kind::Parking => self.stack.push(Task::Pot { p }),
            Kind::GoToJail => self.stack.push(Task::Jail),
            Kind::Market => {
                let i = self.rng.below(MARKET.len()) as u8;
                let (name, delta) = MARKET[i as usize];
                self.emit(ev("market", |j| { j.u("p", p).s("name", name).n("delta", delta as i64); }));
                self.stack.push(Task::Market { i });
            }
            Kind::Jail => {
                self.emit(ev("visitjail", |j| { j.u("p", p); }));
            }
            Kind::Start => {}
        }
    }

    fn draw(&mut self, deck: u8) {
        let d = deck as usize;
        let card = self.decks[d].remove(0);
        self.decks[d].push(card);
        let (emoji, text) = if d == 0 { CHANCE[card as usize] } else { TREASURE[card as usize] };
        let p = self.cur;
        self.emit(ev("card", |j| {
            j.u("p", p).n("deck", deck as i64).n("i", card as i64).s("e", emoji).s("text", text);
        }));
        self.stack.push(Task::CardRun { deck, card });
    }

    fn card_effect(&mut self, deck: u8, card: u8) {
        let p = self.cur;
        let lv = self.levels_sum(p);
        let push = |s: &mut Self, t: Task| s.stack.push(t);
        if deck == 0 {
            match card {
                0 => { push(self, Task::Gain { p, amount: 200 }); push(self, Task::Teleport { cell: self.base[0] }); }
                1 => { push(self, Task::Resolve); push(self, Task::Teleport { cell: self.base[2] + 4 }); }
                2 => push(self, Task::Jail),
                3 => { push(self, Task::Resolve); push(self, Task::Walk { steps: 3, back: true }); }
                4 => {
                    let ring = self.players[p].ring as usize;
                    let len = self.len[ring];
                    let idx = self.players[p].idx as usize;
                    let mut best = 99usize;
                    for s in 0..4 {
                        let g = self.gate[ring][s];
                        let gi = g - self.base[ring];
                        let raw = (gi + len - idx) % len;
                        let d = if raw == 0 { 99 } else { raw };
                        if d < best { best = d; }
                    }
                    push(self, Task::Resolve);
                    push(self, Task::Walk { steps: best as u8, back: false });
                }
                5 => {
                    let list = self.others(p);
                    push(self, Task::PayEach { list, i: 0, amount: 25, incoming: true });
                }
                6 => push(self, Task::Pay { from: p, amount: 50, to: Cred::Pot }),
                7 => { if lv > 0 { push(self, Task::Pay { from: p, amount: lv * 25, to: Cred::Pot }); } }
                8 => push(self, Task::Gain { p, amount: 50 }),
                9 => { push(self, Task::Resolve); push(self, Task::Teleport { cell: self.base[1] + 15 }); }
                10 => push(self, Task::Free),
                11 => { push(self, Task::Resolve); push(self, Task::Teleport { cell: self.base[0] + 23 }); }
                _ => {}
            }
        } else {
            match card {
                0 => push(self, Task::Gain { p, amount: 100 }),
                1 => push(self, Task::Gain { p, amount: 50 }),
                2 => push(self, Task::Pay { from: p, amount: 100, to: Cred::Pot }),
                3 => push(self, Task::Pay { from: p, amount: 50, to: Cred::Pot }),
                4 => push(self, Task::Gain { p, amount: 150 }),
                5 => push(self, Task::Gain { p, amount: 75 }),
                6 => { if lv > 0 { push(self, Task::Pay { from: p, amount: lv * 20, to: Cred::Pot }); } }
                7 => {
                    let list = self.others(p);
                    push(self, Task::PayEach { list, i: 0, amount: 25, incoming: false });
                }
                8 => push(self, Task::Gain { p, amount: 25 }),
                9 => push(self, Task::Pot { p }),
                _ => {}
            }
        }
    }

    fn do_buy(&mut self, cell: usize) {
        let p = self.cur;
        let bot = self.players[p].bot;
        if !bot && !self.ans_ready {
            self.wait_cell = cell;
            self.ask(Wait::Buy, Task::Buy { cell });
            return;
        }
        let want = if bot { self.bot_buy(p, cell) } else { self.ans_b };
        self.ans_ready = false;
        let c = self.c(cell);
        if want && self.players[p].cash >= c.price as i64 {
            self.est[cell].owner = Some(p);
            self.players[p].cash -= c.price as i64;
            self.emit(ev("buy", |j| { j.u("p", p).u("cell", cell).n("price", c.price as i64); }));
            if c.kind == Kind::Prop && self.full_group(p, c.group) {
                let g = c.group;
                self.emit(ev("monopoly", |j| { j.u("p", p).n("group", g as i64); }));
            }
        } else {
            self.emit(ev("pass", |j| { j.u("p", p).u("cell", cell).b("poor", want); }));
        }
    }

    fn do_gate(&mut self) {
        let p = self.cur;
        let bot = self.players[p].bot;
        if !bot && !self.ans_ready {
            self.ask(Wait::Gate, Task::Gate);
            return;
        }
        let dir = if bot { self.bot_gate(p) } else { self.ans_i };
        self.ans_ready = false;
        if dir == 0 {
            self.emit(ev("gatestay", |j| { j.u("p", p); }));
            return;
        }
        let ring = self.players[p].ring as i64;
        let cur_cell = self.cell_of(p);
        let s = self.c(cur_cell).side as usize;
        let nr = ring + dir;
        if nr < 0 || nr > 2 { return; }
        let target = self.gate[nr as usize][s];
        let tc = self.c(target);
        self.players[p].ring = tc.ring;
        self.players[p].idx = tc.idx;
        self.emit(ev("gate", |j| { j.u("p", p).u("cell", target).n("dir", dir); }));
        
        // Исправлено: резолвим клетку назначения
        self.stack.push(Task::Resolve);
    }

    fn do_pay(&mut self, from: usize, amount: u32, to: Cred) {
        if amount == 0 || self.players[from].bankrupt || self.over { return; }
        if let Cred::Player(o) = to {
            if self.players[o].bankrupt { return; }
        }
        if self.players[from].cash >= amount as i64 {
            self.transfer(from, amount, to);
            return;
        }
        if self.players[from].bot {
            self.liquidate(from, amount);
            if self.players[from].cash >= amount as i64 {
                self.transfer(from, amount, to);
            } else {
                self.bankrupt(from, to);
            }
            return;
        }
        self.emit(ev("debt", |j| { j.u("p", from).n("amount", amount as i64).n("to", to.code()); }));
        self.debt_amount = amount;
        self.debt_to = to;
        self.debt_who = from;
        self.ask(Wait::Debt, Task::Pay { from, amount, to });
    }

    fn transfer(&mut self, from: usize, amount: u32, to: Cred) {
        self.players[from].cash -= amount as i64;
        match to {
            Cred::Bank => {}
            Cred::Pot => self.pot += amount,
            Cred::Player(o) => self.players[o].cash += amount as i64,
        }
        let cell = self.cell_of(from);
        self.emit(ev("pay", |j| { j.u("p", from).n("amount", amount as i64).n("to", to.code()).u("cell", cell); }));
    }

    fn bankrupt(&mut self, p: usize, to: Cred) {
        let creditor = match to {
            Cred::Player(o) if !self.players[o].bankrupt && o != p => Some(o),
            _ => None,
        };
        for i in 0..self.cells.len() {
            if self.est[i].owner != Some(p) { continue; }
            let c = self.c(i);
            if self.est[i].level > 0 {
                self.players[p].cash += self.est[i].level as i64 * c.house as i64 / 2;
                self.est[i].level = 0;
            }
            self.est[i].owner = creditor;
        }
        if let Some(o) = creditor {
            let left = self.players[p].cash.max(0);
            self.players[o].cash += left;
        }
        self.players[p].cash = 0;
        self.players[p].bankrupt = true;
        self.players[p].jail = 0;
        self.players[p].jail_free = false;
        self.emit(ev("bankrupt", |j| { j.u("p", p).n("to", creditor.map(|o| o as i64).unwrap_or(-1)); }));
        self.check_win();
    }

    fn check_win(&mut self) {
        let alive: Vec<usize> = (0..self.players.len()).filter(|&i| !self.players[i].bankrupt).collect();
        if alive.len() <= 1 {
            self.over = true;
            self.winner = alive.first().copied();
            let w = self.winner;
            self.emit(ev("win", |j| { j.key("p").raw(&w.map(|x| x.to_string()).unwrap_or_else(|| "null".into())); }));
            self.set_wait(Wait::Over);
        }
    }

    fn next_turn(&mut self) {
        if self.over { self.set_wait(Wait::Over); return; }
        let n = self.players.len();
        for _ in 0..n {
            self.cur = (self.cur + 1) % n;
            if !self.players[self.cur].bankrupt { break; }
        }
        self.begin_turn();
    }

    fn begin_turn(&mut self) {
        self.stack.clear();
        self.turn_no += 1;
        self.chain = 0;
        self.again = false;
        self.ans_ready = false;
        let p = self.cur;
        let no = self.turn_no as i64;
        self.emit(ev("turn", |j| { j.u("p", p).n("no", no); }));
        if self.players[p].jail > 0 {
            self.set_wait(Wait::Jail);
        } else {
            self.set_wait(Wait::Roll);
        }
    }

fn fresh(&mut self) {
    let seed = self.rng.next();
    let n = self.players.len();
    self.players = (0..n).map(|_| Player {
        cash: START_CASH, ring: 0, idx: 0, jail: 0,
        jail_free: false, bankrupt: false, bot: true,   // on_join вернёт людей
    }).collect();
    self.est = vec![Estate::default(); self.cells.len()];
    let mut chance: Vec<u8> = (0..CHANCE.len() as u8).collect();
    let mut treasure: Vec<u8> = (0..TREASURE.len() as u8).collect();
    self.rng.shuffle(&mut chance);
    self.rng.shuffle(&mut treasure);
    self.decks = [chance, treasure];
    self.cur = 0; self.turn_no = 0; self.pot = 0; self.last_sum = 7;
    self.chain = 0; self.again = false;
    self.started = false; self.over = false; self.winner = None;
    self.stack.clear(); self.events.clear();
    self.wait_cell = 0; self.wait_deck = 0; self.wait_card = 0; self.wait_market = 0;
    self.debt_amount = 0; self.debt_to = Cred::Bank; self.debt_who = 0;
    self.ans_b = false; self.ans_i = 0; self.ans_ready = false;
    self.bot_at = 0;
    self.epoch = seed;
    self.set_wait(Wait::Lobby);
}

    // -------------------------------------------------------------- бросок
    fn do_roll(&mut self) {
        let p = self.cur;
        let a = self.rng.die();
        let b = self.rng.die();
        let seed = self.rng.0.to_string();
        self.last_sum = a + b;
        self.emit(ev("dice", |j| { j.u("p", p).n("a", a as i64).n("b", b as i64).s("seed", &seed); }));
        let sum = a + b;
        let dbl = a == b;
        let in_jail = self.players[p].jail > 0;

        if in_jail {
            if dbl {
                self.players[p].jail = 0;
                self.again = false;
                self.emit(ev("jailout", |j| { j.u("p", p).s("how", "double"); }));
                self.push_move(sum);
            } else if self.players[p].jail >= 3 {
                // Исправлено: используем новый таск ForcedJailMove
                self.again = false;
                // Сначала оплата, потом движение (стек LIFO)
                self.stack.push(Task::ForcedJailMove { steps: sum });
                self.stack.push(Task::Pay { from: p, amount: JAIL_FEE, to: Cred::Bank });
            } else {
                self.players[p].jail += 1;
                let n = self.players[p].jail as i64;
                self.again = false;
                self.emit(ev("jailstay", |j| { j.u("p", p).n("n", n); }));
                self.stack.push(Task::AfterRoll);
            }
        } else if dbl {
            self.chain += 1;
            if self.chain >= 3 {
                self.again = false;
                self.emit(ev("triple", |j| { j.u("p", p); }));
                self.stack.push(Task::AfterRoll);
                self.stack.push(Task::Jail);
            } else {
                self.again = true;
                self.push_move(sum);
            }
        } else {
            self.chain = 0;
            self.again = false;
            self.push_move(sum);
        }
        self.set_wait(Wait::Run);
        self.run();
    }

    fn push_move(&mut self, sum: u8) {
        self.stack.push(Task::AfterRoll);
        self.stack.push(Task::Resolve);
        self.stack.push(Task::Walk { steps: sum, back: false });
    }

    // -------------------------------------------------------------- боты
    fn reserve(&self) -> i64 {
        match self.diff {
            Diff::Aggressive => 120, Diff::Careful => 420, Diff::Balanced => 260,
        }
    }

    fn bot_buy(&self, p: usize, cell: usize) -> bool {
        let c = self.c(cell);
        let cash = self.players[p].cash;
        let r = self.reserve();
        if c.kind == Kind::Utility {
            return cash - c.price as i64 > (r as f64 * 0.7) as i64;
        }
        let g = &self.group_cells[c.group as usize];
        let mine = g.iter().filter(|&&i| self.est[i].owner == Some(p)).count();
        let completes = mine + 1 == g.len();
        let floor = if completes { (r as f64 * 0.4) as i64 } else { r };
        cash - c.price as i64 > floor
    }

    fn bot_gate(&mut self, p: usize) -> i64 {
        let mut own = [0f64; 3];
        for (i, c) in self.cells.iter().enumerate() {
            if (c.kind == Kind::Prop || c.kind == Kind::Utility) && self.est[i].owner == Some(p) {
                own[c.ring as usize] += 1.0;
            }
        }
        let cash = self.players[p].cash;
        let ring = self.players[p].ring as i64;
        let jitter = (self.rng.below(1000) as f64) / 1000.0;
        let score = |r: i64| -> f64 {
            own[r as usize] * 2.0
                + if r == 0 && cash < 150 { 2.0 } else { 0.0 }
                + if r == 2 && cash > 600 { 1.5 } else { 0.0 }
        };
        let mut best = 0i64;
        let mut bs = score(ring) + jitter;
        for d in [1i64, -1] {
            let r = ring + d;
            if r < 0 || r > 2 { continue; }
            let j2 = (self.rng.below(1000) as f64) / 1000.0;
            let s = score(r) + j2;
            if s > bs { bs = s; best = d; }
        }
        best
    }

    fn bot_jail(&self, p: usize) -> &'static str {
        let pl = &self.players[p];
        if pl.jail_free { "card" }
        else if pl.cash > 400 || pl.jail >= 2 { "pay" }
        else { "roll" }
    }

    fn bot_build(&mut self, p: usize) {
        for _ in 0..2 {
            let r = self.reserve();
            let cand: Vec<usize> = (0..self.cells.len())
                .filter(|&i| self.can_build(p, i) && self.players[p].cash - self.c(i).house as i64 >= r)
                .collect();
            if cand.is_empty() { break; }
            let cell = *cand.iter().max_by_key(|&&i| self.c(i).rents[0]).unwrap();
            self.build(p, cell);
        }
    }

    fn liquidate(&mut self, p: usize, need: u32) {
        let mut guard = 0;
        while self.players[p].cash < need as i64 && guard < 200 {
            guard += 1;
            let with_b: Vec<usize> = (0..self.cells.len()).filter(|&i| self.can_sell_b(p, i)).collect();
            if !with_b.is_empty() {
                let cell = *with_b.iter().min_by_key(|&&i| self.c(i).rents[0]).unwrap();
                self.sell_b(p, cell);
                continue;
            }
            let sellable: Vec<usize> = (0..self.cells.len()).filter(|&i| self.can_sell_p(p, i)).collect();
            if sellable.is_empty() { break; }
            let cell = *sellable
                .iter()
                .min_by_key(|&&i| {
                    let c = self.c(i);
                    let mono = if c.kind == Kind::Prop && self.full_group(p, c.group) { 1 } else { 0 };
                    (mono, c.price)
                })
                .unwrap();
            self.sell_p(p, cell);
        }
    }

    // -------------------------------------------------------------- активы
    fn build(&mut self, p: usize, cell: usize) {
        let c = self.c(cell);
        self.players[p].cash -= c.house as i64;
        self.est[cell].level += 1;
        let lv = self.est[cell].level as i64;
        self.emit(ev("build", |j| { j.u("p", p).u("cell", cell).n("level", lv); }));
    }
    fn sell_b(&mut self, p: usize, cell: usize) {
        let c = self.c(cell);
        self.est[cell].level -= 1;
        self.players[p].cash += c.house as i64 / 2;
        let lv = self.est[cell].level as i64;
        self.emit(ev("sellb", |j| { j.u("p", p).u("cell", cell).n("level", lv); }));
    }
    fn sell_p(&mut self, p: usize, cell: usize) {
        let c = self.c(cell);
        self.est[cell].owner = None;
        self.est[cell].level = 0;
        self.players[p].cash += c.price as i64 / 2;
        self.emit(ev("sellp", |j| { j.u("p", p).u("cell", cell); }));
    }

    // -------------------------------------------------------------- команды
    fn act(&mut self, seat: usize, cmd: &Cmd) -> Result<(), String> {
        let cell_arg = |s: &Self, cmd: &Cmd| -> Result<usize, String> {
            let v = cmd.num("cell").ok_or("Не указана клетка")?;
            if v < 0 || v as usize >= s.cells.len() { return Err("Нет такой клетки".into()); }
            Ok(v as usize)
        };
        match cmd.id {
            "roll" => {
                if self.wait != Wait::Roll && self.wait != Wait::Jail { return Err("Сейчас нельзя бросать".into()); }
                self.do_roll();
                Ok(())
            }
            "card" => {
                if self.wait != Wait::Jail || !self.players[seat].jail_free { return Err("Нет карты «Освобождение»".into()); }
                self.players[seat].jail_free = false;
                self.players[seat].jail = 0;
                self.emit(ev("jailout", |j| { j.u("p", seat).s("how", "card"); }));
                self.set_wait(Wait::Roll);
                Ok(())
            }
            "pay" => {
                if self.wait == Wait::Jail {
                    if self.players[seat].cash < JAIL_FEE as i64 { return Err("Не хватает ₽50".into()); }
                    self.stack.push(Task::Unjail);
                    self.stack.push(Task::Pay { from: seat, amount: JAIL_FEE, to: Cred::Bank });
                    self.set_wait(Wait::Run);
                    self.run();
                    return Ok(());
                }
                if self.wait == Wait::Debt {
                    if self.players[seat].cash < self.debt_amount as i64 { return Err("Ещё не хватает — продавай дальше".into()); }
                    self.answer();
                    return Ok(());
                }
                Err("Сейчас нечего оплачивать".into())
            }
            "buy" | "pass" => {
                if self.wait != Wait::Buy { return Err("Сейчас нечего покупать".into()); }
                self.ans_b = cmd.id == "buy";
                self.answer();
                Ok(())
            }
            "gate" => {
                if self.wait != Wait::Gate { return Err("Ты не на Воротах".into()); }
                let d = cmd.num("dir").unwrap_or(0);
                if ![-1, 0, 1].contains(&d) { return Err("Битое направление".into()); }
                let r = self.players[seat].ring as i64 + d;
                if r < 0 || r > 2 { return Err("Туда нет перехода".into()); }
                self.ans_i = d;
                self.answer();
                Ok(())
            }
            "ok" => {
                if self.wait != Wait::Card && self.wait != Wait::Market { return Err("Нечего подтверждать".into()); }
                self.answer();
                Ok(())
            }
            "end" => {
                if self.wait != Wait::End { return Err("Ход ещё не закончен".into()); }
                self.next_turn();
                Ok(())
            }
            "build" => {
                if !self.manage_ok() { return Err("Стройка доступна до броска или после него".into()); }
                let cell = cell_arg(self, cmd)?;
                if !self.can_build(seat, cell) { return Err("Здесь строить нельзя".into()); }
                self.build(seat, cell);
                Ok(())
            }
            "sellb" => {
                if !self.manage_ok() && self.wait != Wait::Debt { return Err("Сейчас нельзя сносить".into()); }
                let cell = cell_arg(self, cmd)?;
                if !self.can_sell_b(seat, cell) { return Err("Сначала снимай верхние уровни группы".into()); }
                self.sell_b(seat, cell);
                Ok(())
            }
            "sellp" => {
                if !self.manage_ok() && self.wait != Wait::Debt { return Err("Сейчас нельзя продавать".into()); }
                let cell = cell_arg(self, cmd)?;
                if !self.can_sell_p(seat, cell) { return Err("Сначала снеси постройки группы".into()); }
                self.sell_p(seat, cell);
                Ok(())
            }
            "bankrupt" => {
                if self.wait != Wait::Debt { return Err("Банкротство доступно только при долге".into()); }
                self.stack.pop();
                let to = self.debt_to;
                self.bankrupt(seat, to);
                self.set_wait(Wait::Run);
                self.run();
                Ok(())
            }
            "concede" => {
                if !self.manage_ok() { return Err("Сдаться можно в свой ход".into()); }
                self.bankrupt(seat, Cred::Bank);
                self.stack.clear();
                if !self.over { self.next_turn(); }
                Ok(())
            }
            _ => Err("Неизвестное действие".into()),
        }
    }

    fn bot_act(&mut self, a: usize) {
        match self.wait {
            Wait::Roll => self.do_roll(),
            Wait::Jail => match self.bot_jail(a) {
                "card" => {
                    self.players[a].jail_free = false;
                    self.players[a].jail = 0;
                    self.emit(ev("jailout", |j| { j.u("p", a).s("how", "card"); }));
                    self.set_wait(Wait::Roll);
                }
                "pay" if self.players[a].cash >= JAIL_FEE as i64 => {
                    self.stack.push(Task::Unjail);
                    self.stack.push(Task::Pay { from: a, amount: JAIL_FEE, to: Cred::Bank });
                    self.set_wait(Wait::Run);
                    self.run();
                }
                _ => self.do_roll(),
            },
            // Исправлено: боты теперь выполняют полные действия, а не просто ставят флаги
            Wait::Buy => {
                self.do_buy(self.wait_cell);
            }
            Wait::Gate => {
                self.do_gate();
            }
            Wait::Card | Wait::Market => self.answer(),
            Wait::Debt => {
                self.set_wait(Wait::Run);
                self.run();
            }
            Wait::End => {
                self.bot_build(a);
                self.next_turn();
            }
            _ => {}
        }
    }

    fn bot_delay(&self) -> u64 {
        match self.wait {
            Wait::Card => 1200, Wait::Market => 1000,
            Wait::Roll | Wait::Jail => 850, _ => 700,
        }
    }
}

// ================================================================== GAME
impl Game for Rings {
    fn id(&self) -> &'static str { "rings" }
    fn seats(&self) -> usize { self.seats }
    fn min_humans(&self) -> usize { 1 }
    fn started(&self) -> bool { self.started }
    fn over(&self) -> bool { self.over }

    fn turn(&self) -> Option<Seat> {
        if !self.started || self.over { None } else { Some(self.actor()) }
    }
    fn is_bot(&self, seat: Seat) -> bool {
        self.players.get(seat).map(|p| p.bot).unwrap_or(true)
    }

    fn begin(&mut self) -> Vec<String> {
        if self.started { return vec![]; }
        self.started = true;
        self.events.clear();
        self.emit(ev("begin", |_| {}));
        self.cur = self.players.len() - 1;
        self.next_turn();
        std::mem::take(&mut self.events)
    }

    fn on_join(&mut self, seat: Seat) -> Vec<String> {
        if let Some(p) = self.players.get_mut(seat) { p.bot = false; }
        self.bot_at = 0;
        if self.started && !self.over {
            vec![ev("botoff", |j| { j.u("p", seat); })]
        } else { vec![] }
    }

    fn on_leave(&mut self, seat: Seat) -> Vec<String> {
        if let Some(p) = self.players.get_mut(seat) { p.bot = true; }
        self.bot_at = 0;
        if self.started && !self.over {
            vec![ev("boton", |j| { j.u("p", seat); })]
        } else { vec![] }
    }

    fn set_name(&mut self, seat: Seat, name: &str) {
        if let Some(n) = self.names.get_mut(seat) {
            let cleaned: String = name.chars().take(16).collect();
            if !cleaned.trim().is_empty() { *n = cleaned; }
        }
    }

fn actions(&self, seat: Seat) -> Vec<Action> {
    let mut a = Vec::new();
    if self.over {
        return a;
    }
    if seat >= self.players.len() || self.players[seat].bankrupt {
        return a;
    }

    let is_actor = self.actor() == seat;
    let bot = self.players[seat].bot;

    // Управление активами доступно и во время долга (продажа/снос)
    self.manage_actions(seat, &mut a);

    match self.wait {
        Wait::Lobby => {}
        Wait::Run => {}
        Wait::Over => {}
        Wait::Roll => {
            if is_actor && !bot {
                a.push(Action::new("roll", "🎲 Бросить"));
            }
        }
        Wait::Jail => {
            if is_actor && !bot {
                a.push(Action::new("roll", "🎲 Бросить (дубль)"));
                if self.players[seat].jail_free {
                    a.push(Action::new("card", "🗝 Карта «Освобождение»"));
                }
                if self.players[seat].cash >= JAIL_FEE as i64 {
                    a.push(Action::new("pay", "💰 Заплатить ₽50"));
                }
            }
        }
        Wait::Buy => {
            if is_actor && !bot && self.wait_cell < self.cells.len() {
                let c = self.c(self.wait_cell);
                if self.players[seat].cash >= c.price as i64 {
                    a.push(Action::with("buy", "🏠 Купить", "cell"));
                }
                a.push(Action::with("pass", "🚶 Пропустить", "cell"));
            }
        }
     Wait::Gate => {
         if is_actor && !bot {
             let ring = self.players[seat].ring as i64;
             for (d, arg) in [(-1i64, "dir=-1"), (0, "dir=0"), (1, "dir=1")] {
                 let r = ring + d;
                 if r < 0 || r > 2 { continue; }
                 let label = match d {
                     -1 => "⬅ Во внешнее кольцо",
                      0 => "⏸ Остаться",
                      _ => "➡ Во внутреннее кольцо",
                 };
                 a.push(Action::arg("gate", label, arg));
             }
         }
     }
        Wait::Card | Wait::Market => {
            if is_actor && !bot {
                a.push(Action::new("ok", "✅ ОК"));
            }
        }
        Wait::Debt => {
            if is_actor && !bot {
                if self.players[seat].cash >= self.debt_amount as i64 {
                    a.push(Action::new("pay", "💰 Оплатить"));
                }
                a.push(Action::new("bankrupt", "🏳 Объявить банкротство"));
            }
        }
        Wait::End => {
            if is_actor && !bot {
                a.push(Action::new("end", "➡ Завершить ход"));
            }
        }
    }

    a
}

    fn apply(&mut self, seat: Seat, cmd: &Cmd) -> Step {
        if !self.started { return Step::no("Партия ещё не началась"); }
        if self.over { return Step::no("Партия окончена — жми «Реванш»"); }
        if seat >= self.players.len() { return Step::no("Нет такого места"); }
        if self.players[seat].bot { return Step::no("Это место играет бот"); }
        if self.players[seat].bankrupt { return Step::no("Ты выбыл из партии"); }
        if seat != self.actor() { return Step::no("Сейчас не твой ход"); }
        self.events.clear();
        match self.act(seat, cmd) {
            Ok(()) => Step::ok(std::mem::take(&mut self.events)),
            Err(e) => { self.events.clear(); Step::no(e) }
        }
    }

    fn tick(&mut self, now: u64) -> Option<Vec<String>> {
        if !self.started || self.over || self.wait == Wait::Run || self.wait == Wait::Lobby {
            return None;
        }
        let a = self.actor();
        if a >= self.players.len() || !self.players[a].bot { return None; }
        if self.bot_at == 0 {
            self.bot_at = now.saturating_add(self.bot_delay());
            return None;
        }
        if now < self.bot_at { return None; }
        self.bot_at = 0;
        self.events.clear();
        self.bot_act(a);
        let e = std::mem::take(&mut self.events);
        if e.is_empty() { None } else { Some(e) }
    }

    fn view(&self, seat: Seat) -> String {
        let mut players = J::arr();
        for (i, p) in self.players.iter().enumerate() {
            let mut j = J::obj();
            let nm = self.names.get(i).map(|s| s.as_str())
                .unwrap_or(BOT_NAMES[i % BOT_NAMES.len()]);
            j.u("i", i)
                .n("cash", p.cash)
                .n("ring", p.ring as i64)
                .n("idx", p.idx as i64)
                .u("cell", self.cell_of(i))
                .n("jail", p.jail as i64)
                .b("free", p.jail_free)
                .b("bankrupt", p.bankrupt)
                .b("bot", p.bot)
                .n("worth", self.net_worth(i))
                .s("color", COLORS[i % COLORS.len()])
                .s("name", nm)
                .s("botname", BOT_NAMES[i % BOT_NAMES.len()]);
            players.push(&j.end());
        }
        let mut props = J::arr();
        for (i, c) in self.cells.iter().enumerate() {
            if c.kind != Kind::Prop && c.kind != Kind::Utility { continue; }
            let mut j = J::obj();
            j.u("c", i).opt("o", self.est[i].owner).n("l", self.est[i].level as i64);
            props.push(&j.end());
        }
        let mut w = J::obj();
        w.s("k", self.wait.as_str());
        match self.wait {
            Wait::Buy => { w.u("cell", self.wait_cell); }
            Wait::Card => { w.n("deck", self.wait_deck as i64).n("card", self.wait_card as i64); }
            Wait::Market => { w.n("i", self.wait_market as i64); }
            Wait::Debt => {
                w.n("amount", self.debt_amount as i64)
                    .n("to", self.debt_to.code())
                    .u("who", self.debt_who);
            }
            _ => {}
        }

        let mut acts = J::arr();
        for a in self.actions(seat) {
            let mut o = J::obj();
            o.s("id", a.id).s("label", a.label).s("arg", a.arg);
            acts.push(&o.end());
        }

        let mut j = J::obj();
        j.s("game", "rings")
            .u("epoch", self.epoch as usize)
            .b("started", self.started)
            .b("over", self.over)
            .opt("winner", self.winner)
            .u("cur", self.cur)
            .opt("actor", self.turn())
            .n("turnNo", self.turn_no as i64)
            .n("pot", self.pot as i64)
            .n("lastSum", self.last_sum as i64)
            .n("chain", self.chain as i64)
            .b("again", self.again)
            .opt("you", if seat == SPECTATOR { None } else { Some(seat) })
            .key("wait").raw(&w.end())
            .key("players").raw(&players.end())
            .key("props").raw(&props.end())
            .key("defs").raw(&self.defs)
            .key("acts").raw(&acts.end());
        j.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(n: usize) -> Rings {
        let s = n.to_string();
        let mut g = Rings::new(&[("seats", &s), ("seed", "42")]);
        for i in 0..n { g.on_join(i); }
        g.begin();
        g
    }

    #[test]
    fn board_shape() {
        let c = make_cells();
        assert_eq!(c.len(), 48);
        assert_eq!(c.iter().filter(|x| x.kind == Kind::Gate).count(), 12);
        assert_eq!(c[6].kind, Kind::Jail);
        assert_eq!(c[44].name, "Башня «Зенит»");
        assert_eq!(c[39].name, "Аэропорт");
        assert_eq!(c[23].kind, Kind::Utility);
    }

    #[test]
    fn rent_math() {
        let c = make_cells();
        assert_eq!(c[1].rents[0], 5);
        assert_eq!(c[1].rents[5], c[1].rents[0] * 36);
    }

    #[test]
    fn even_building() {
        let mut g = game(2);
        for &i in &g.group_cells[0].clone() { g.est[i].owner = Some(0); }
        let first = g.group_cells[0][0];
        let second = g.group_cells[0][1];
        assert!(g.can_build(0, first));
        g.build(0, first);
        assert!(!g.can_build(0, first));
        assert!(g.can_build(0, second));
        assert!(!g.can_sell_p(0, second));
        assert!(g.can_sell_b(0, first));
    }

    #[test]
    fn bankruptcy_transfers() {
        let mut g = game(2);
        let cell = g.group_cells[0][0];
        g.est[cell].owner = Some(0);
        g.players[0].cash = 10;
        g.bankrupt(0, Cred::Player(1));
        assert!(g.players[0].bankrupt);
        assert_eq!(g.est[cell].owner, Some(1));
        assert!(g.over);
        assert_eq!(g.winner, Some(1));
    }

    /// Регресс на главный баг: у человека всегда есть чем закончить ход.
    #[test]
    fn human_can_always_finish_turn() {
        let mut g = game(2);
        for _ in 0..400 {
            if g.over { break; }
            let a = g.actor();
            let acts = g.actions(a);
            assert!(!acts.is_empty(), "тупик: wait={:?} seat={}", g.wait, a);
            let id = if acts.iter().any(|x| x.id == "end") { "end" }
                     else if acts.iter().any(|x| x.id == "roll") { "roll" }
                     else if acts.iter().any(|x| x.id == "ok") { "ok" }
                     else if acts.iter().any(|x| x.id == "pass") { "pass" }
                     else if acts.iter().any(|x| x.id == "pay") { "pay" }
                     else { acts[0].id };
            let args = if id == "pass" { vec![("cell", "0")] } else { vec![] };
            let step = g.apply(a, &Cmd { id, args });
            assert!(matches!(step, Step::Ok { .. }), "{id} отклонено в {:?}", g.wait);
        }
    }

    #[test]
    fn bots_play_a_full_game() {
        let mut g = Rings::new(&[("seats", "4"), ("seed", "7")]);
        g.begin();
        let mut now = 0u64;
        for _ in 0..200_000 {
            if g.over { break; }
            now += 250;
            g.tick(now);
        }
        assert!(g.over, "партия не закончилась за отведённые тики");
    }
}