use crate::game::Game;
use crate::Client;
use std::sync::Arc;

pub struct Room {
    pub code: String,
    pub game_id: String,
    pub game: Box<dyn Game>,
    pub players: Vec<Option<Arc<Client>>>,
    pub tokens: Vec<String>,
    pub names: Vec<String>,
    pub seq: u64,
    pub host: Option<usize>,
}

impl Room {
    pub fn publish(&mut self, events: Vec<String>) {
        if self.players.iter().all(|p| p.is_none()) { return; }
        self.seq += 1;
        for (s, p) in self.players.iter().enumerate() {
            let Some(c) = p else { continue };
            if !c.alive() { continue; }
            c.send(&format!("view {} {} {}", self.seq, s, self.game.view(s)));
            for e in &events {
                c.send(&format!("ev {} {}", self.seq, e));
            }
        }
    }

    pub fn broadcast_views(&mut self) {
        if self.players.iter().all(|p| p.is_none()) { return; }
        self.seq += 1;
        for (s, p) in self.players.iter().enumerate() {
            let Some(c) = p else { continue };
            if !c.alive() { continue; }
            c.send(&format!("view {} {} {}", self.seq, s, self.game.view(s)));
        }
    }

    fn view_for(&self, seat: usize) -> String {
        let v = self.game.view(seat);
        let mut acts = crate::game::J::arr();
        for a in self.game.actions(seat) {
            let mut o = crate::game::J::obj();
            o.s("id", a.id).s("label", a.label).s("arg", a.arg);
            acts.push(&o.end());
        }
        let body = v.trim_end();
        match body.strip_suffix('}') {
            Some(head) => {
                let sep = if head.trim_end().ends_with('{') { "" } else { "," };
                format!("{head}{sep}\"acts\":{}}}", acts.end())
            }
            None => v,
        }
    }

    /// JSON-описание комнаты для списка игр
    pub fn summary_json(&self) -> String {
        let humans = self.players.iter().filter(|p| p.is_some()).count();
        let mut players = crate::game::J::arr();
        for (i, p) in self.players.iter().enumerate() {
            let mut o = crate::game::J::obj();
            o.u("seat", i)
                .b("connected", p.is_some())
                .b("bot", self.game.is_bot(i))
                .s("name", self.names.get(i).map(|s| s.as_str()).unwrap_or(""))
                .b("host", self.host == Some(i));
            players.push(&o.end());
        }
        let mut j = crate::game::J::obj();
        j.s("code", &self.code)
            .s("game", &self.game_id)
            .opt("host", self.host)
            .u("seats", self.players.len())
            .u("humans", humans)
            .b("started", self.game.started())
            .b("over", self.game.over())
            .opt("turn", self.game.turn())
            .u("seq", self.seq as usize)
            .key("players")
            .raw(&players.end());
        j.end()
    }
}