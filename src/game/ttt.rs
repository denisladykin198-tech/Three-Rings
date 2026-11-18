//! Крестики-нолики как плагин. Правила те же, что были вшиты в main.rs,
//! но теперь сервер про них не знает — хороший регресс-тест каркаса.

use super::{ev, Action, Cmd, Game, J, Seat, Step, SPECTATOR};

const LINES: [[usize; 3]; 8] = [
    [0, 1, 2], [3, 4, 5], [6, 7, 8],
    [0, 3, 6], [1, 4, 7], [2, 5, 8],
    [0, 4, 8], [2, 4, 6],
];

pub struct Ttt {
    board: [Option<usize>; 9], // индекс места = X/O
    turn: usize,
    winner: Option<usize>,
    line: Option<[usize; 3]>,
    draw: bool,
    started: bool,
}

impl Ttt {
    pub fn new() -> Self {
        Self {
            board: [None; 9],
            turn: 0,
            winner: None,
            line: None,
            draw: false,
            started: false,
        }
    }
    fn place(&mut self, cell: usize, seat: usize) {
        self.board[cell] = Some(seat);
        for l in LINES {
            if l.iter().all(|&i| self.board[i] == Some(seat)) {
                self.winner = Some(seat);
                self.line = Some(l);
                return;
            }
        }
        if self.board.iter().all(|c| c.is_some()) {
            self.draw = true;
        } else {
            self.turn = 1 - seat;
        }
    }
}

impl Game for Ttt {
    fn id(&self) -> &'static str { "ttt" }
    fn seats(&self) -> usize { 2 }
    fn min_humans(&self) -> usize { 2 }
    fn started(&self) -> bool { self.started }
    fn over(&self) -> bool { self.winner.is_some() || self.draw }
    fn turn(&self) -> Option<Seat> { if self.over() { None } else { Some(self.turn) } }
    fn is_bot(&self, _: Seat) -> bool { false }

    fn begin(&mut self) -> Vec<String> {
        if self.started { return vec![] }
        self.started = true;
        vec![ev("begin", |_| {})]
    }

    fn actions(&self, seat: Seat) -> Vec<Action> {
        if !self.started || self.over() || seat != self.turn {
            return vec![];
        }
        (0..9)
            .filter(|&i| self.board[i].is_none())
            .map(|i| match i {
                0 => Action::with("move", "↖", "cell=0"),
                1 => Action::with("move", "↑", "cell=1"),
                2 => Action::with("move", "↗", "cell=2"),
                3 => Action::with("move", "←", "cell=3"),
                4 => Action::with("move", "·", "cell=4"),
                5 => Action::with("move", "→", "cell=5"),
                6 => Action::with("move", "↙", "cell=6"),
                7 => Action::with("move", "↓", "cell=7"),
                _ => Action::with("move", "↘", "cell=8"),
            })
            .collect()
    }

    fn apply(&mut self, seat: Seat, cmd: &Cmd) -> Step {
        if cmd.id != "move" {
            return Step::no("Неизвестное действие");
        }
        if !self.started { return Step::no("Партия ещё не началась") }
        if self.over() { return Step::no("Игра окончена — жми «Реванш»") }
        if seat != self.turn { return Step::no("Сейчас не твой ход") }
        let Some(cell) = cmd.num("cell") else { return Step::no("Битый ход") };
        if !(0..9).contains(&cell) || self.board[cell as usize].is_some() {
            return Step::no("Недопустимый ход");
        }
        self.place(cell as usize, seat);
        let mut e = vec![ev("move", |j| { j.u("p", seat).u("cell", cell as usize); })];
        if let Some(w) = self.winner {
            e.push(ev("win", |j| { j.u("p", w); }));
        } else if self.draw {
            e.push(ev("draw", |_| {}));
        }
        Step::ok(e)
    }

    fn view(&self, seat: Seat) -> String {
        let cells: Vec<String> = self
            .board
            .iter()
            .map(|c| match c {
                Some(0) => "\"X\"".into(),
                Some(_) => "\"O\"".into(),
                None => "null".into(),
            })
            .collect();
        let mut j = J::obj();
        j.s("game", "ttt")
            .b("started", self.started)
            .b("over", self.over())
            .list("board", &cells)
            .opt("turn", self.turn())
            .opt("winner", self.winner)
            .b("draw", self.draw)
            .opt("you", if seat == SPECTATOR { None } else { Some(seat) });
        if let Some(l) = self.line {
            j.nums("line", &[l[0] as i64, l[1] as i64, l[2] as i64]);
        }
        j.end()
    }
}