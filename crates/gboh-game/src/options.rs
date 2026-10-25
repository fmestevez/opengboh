//! Rule options: the manuals' rules where the games' code differs in ways
//! that change the balance, and rules of the later games brought back to
//! the earlier ones. All off by default (the games as they shipped, their
//! plain bugs fixed). A save records the options its battle was fought by.

/// Each option: its key (in settings and saves), and its wording.
pub const ALL: [(&str, &str); 14] = [
    ("manual_turning", "Manual: elephants and chariots (and heavy horse) pay 2 MP to turn"),
    ("manual_shock", "Manual: shock with terrain shifts, its charge, size ratio and leader rules"),
    ("manual_missiles", "Manual: woods protect, replenishment, 15% leader hits"),
    ("manual_movement", "Manual: leaving zones of control, skirmishers' flank zone"),
    ("manual_command", "Manual: group order odds, leaders' withdrawal risk"),
    ("manual_victory", "Manual: rout points, turn limits and battles' special rules"),
    ("pursuit", "Alexander: cavalry pursue broken units (from Hannibal)"),
    ("deployment", "Alexander: deploy before the battle (from Hannibal)"),
    ("backward", "Alexander: backward movement (from Hannibal)"),
    ("group_orders", "Alexander: group retreat, attack, restore cohesion (from Hannibal)"),
    ("roads", "Alexander, Hannibal: road to road costs 1 MP (from Caesar)"),
    ("rally_mark", "Alexander, Hannibal: rallied units stay shaken (from Caesar)"),
    ("staff_leaders", "Alexander, Hannibal: staff leaders command at no extra cost (from Caesar)"),
    ("no_stacked_superiority", "Hannibal: superiority doesn't stack (from Caesar)"),
];

/// The options in force (see [`ALL`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    pub manual_turning: bool,
    pub manual_shock: bool,
    pub manual_missiles: bool,
    pub manual_movement: bool,
    pub manual_command: bool,
    pub manual_victory: bool,
    pub pursuit: bool,
    pub deployment: bool,
    pub backward: bool,
    pub group_orders: bool,
    pub roads: bool,
    pub rally_mark: bool,
    pub staff_leaders: bool,
    pub no_stacked_superiority: bool,
}

impl Options {
    /// Option `key`'s flag.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut bool> {
        Some(match key {
            "manual_turning" => &mut self.manual_turning,
            "manual_shock" => &mut self.manual_shock,
            "manual_missiles" => &mut self.manual_missiles,
            "manual_movement" => &mut self.manual_movement,
            "manual_command" => &mut self.manual_command,
            "manual_victory" => &mut self.manual_victory,
            "pursuit" => &mut self.pursuit,
            "deployment" => &mut self.deployment,
            "backward" => &mut self.backward,
            "group_orders" => &mut self.group_orders,
            "roads" => &mut self.roads,
            "rally_mark" => &mut self.rally_mark,
            "staff_leaders" => &mut self.staff_leaders,
            "no_stacked_superiority" => &mut self.no_stacked_superiority,
            _ => return None,
        })
    }

    pub fn get(&self, key: &str) -> bool {
        let mut me = *self;
        me.get_mut(key).is_some_and(|f| *f)
    }

    /// The keys of the options on, space-separated.
    pub fn to_text(&self) -> String {
        ALL.iter().filter(|(k, _)| self.get(k)).map(|(k, _)| *k).collect::<Vec<_>>().join(" ")
    }

    /// Options from [`Options::to_text`] (unknown keys ignored).
    pub fn parse(text: &str) -> Self {
        let mut o = Self::default();
        for k in text.split_whitespace() {
            if let Some(f) = o.get_mut(k) {
                *f = true;
            }
        }
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_round_trip() {
        let o = Options { roads: true, pursuit: true, ..Default::default() };
        assert_eq!(o.to_text(), "pursuit roads");
        assert_eq!(Options::parse(&o.to_text()), o);
        assert_eq!(Options::parse("nonsense"), Options::default());
    }
}
