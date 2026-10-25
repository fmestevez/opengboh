//! The scenario editor's model: a *Collector's Edition* scenario (see
//! `gboh_formats::scn::Layout::Editor`) and its map, edited by the rules of
//! the original editor (`scenario editor.exe`): terrain painted with no two
//! neighbours more than a level apart; leaders forming the groups their
//! units join, every unit fighting for one of the two sides; and, on
//! saving, each side's AI plan written from its groups' orders.

use crate::battle::flank;
use crate::hex::Hex;
use gboh_formats::map::{Map, Side as MapSide};
use gboh_formats::scn::{EditorInfo, Group, Layout, Scenario, Side, Unit, UnitExtra};

/// The editor's unit kinds (the record a unit is written with).
pub const KINDS: [&str; 18] = [
    "Leader",
    "Artillery",
    "Heavy Cavalry",
    "Light Cavalry",
    "Chariot",
    "Elephant",
    "Heavy Infantry",
    "Light Infantry",
    "Medium Infantry",
    "Lancer",
    "Skirmisher",
    "Oxybeles",
    "Phalanx",
    "Legionnaire",
    "Roman Cavalry",
    "Barbarian Infantry",
    "Barbarian Cavalry",
    "Cohort",
];

/// The nations, by the scenarios' nation byte.
pub const NATIONS: [&str; 12] =
    ["Greek", "Persian", "Macedonian", "Indian", "Danubian", "Scythian", "Roman", "Carthaginian", "Seleucid", "Caesarian", "Pompeian", "Barbarian"];

/// What the map brush paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    /// A land level, 1..=9.
    Level(u8),
    /// Water (level 0).
    Water,
    /// The hex's ground (its codes cleared first): woods, rough ground, a
    /// road; or clear ground.
    Woods,
    Rocky,
    Road,
    Clear,
}

/// Map hex codes the editor paints.
const WOODS: u16 = 1;
const ROCKY: u16 = 2;
const ROAD: u16 = 12;
/// Side codes: a stream, a wall.
pub const STREAM: u16 = 6;
pub const WALL: u16 = 9;

/// The largest map side, in hexes. The original editor stops at 50, but
/// the games' own maps reach 89 (Trasimene's width) and 85 (Ilipa's
/// height), and play.
pub const MAX_SIZE: usize = 89;

/// A group's order: type (1 defend, 2 move, 3 attack), turns, hex.
type Order = (u32, u16, u16);

/// A scenario being edited, with its map.
pub struct Editor {
    pub scn: Scenario,
    pub map: Map,
    /// The next leader's group id (the original forgets it on opening a
    /// file and reuses ids: here the highest plus one).
    next_group: i16,
}

impl Editor {
    /// A new scenario on `map`.
    pub fn new(map: Map, map_file: &str) -> Self {
        let side = |retreat| Side { nation: 255, retreat, rout_level: 100 };
        let scn = Scenario {
            layout: Layout::Editor,
            leaders: Vec::new(),
            unk: 0,
            title: "New battle".into(),
            map_file: map_file.into(),
            unit_file: String::new(),
            home: side(1),
            visitor: side(5),
            pairs: Vec::new(),
            home_ai: Vec::new(),
            visitor_ai: Vec::new(),
            units: Vec::new(),
            editor: Some(EditorInfo::default()),
        };
        Self { scn, map, next_group: 0 }
    }

    /// An existing scenario and its map (a game's scenario is taken over
    /// as an editor one: its units keep their art and stats).
    pub fn open(mut scn: Scenario, map: Map) -> Self {
        if scn.layout != Layout::Editor {
            convert(&mut scn);
        }
        let next_group = scn.editor.as_ref().map_or(0, |e| e.groups.iter().map(|g| g.id + 1).max().unwrap_or(0));
        Self { scn, map, next_group }
    }

    fn info(&mut self) -> &mut EditorInfo {
        self.scn.editor.get_or_insert_with(EditorInfo::default)
    }

    /// The hex id of map hex `(x, y)`.
    pub fn hex_id(&self, x: usize, y: usize) -> u16 {
        self.map.hex_id_origin + (x * 100 + y) as u16
    }

    /// The units' hexes on the map (with two-hex units' second ones).
    fn taken(&self, except: Option<usize>) -> Vec<Hex> {
        let mut out = Vec::new();
        for (i, u) in self.scn.units.iter().enumerate() {
            if Some(i) == except {
                continue;
            }
            if let Some((x, y)) = self.map.hex_from_id(u.hex) {
                let h = Hex::new(x as i32, y as i32);
                out.push(h);
                if two_hex(u)
                    && let Some(s) = flank(h, u.facing)
                {
                    out.push(s);
                }
            }
        }
        out
    }

    fn on_map(&self, h: Hex) -> bool {
        h.x >= 0 && h.y >= 0 && (h.x as usize) < self.map.width && (h.y as usize) < self.map.height
    }

    /// Paint hex `(x, y)`. A level is refused next to a hex more than one
    /// level away (as the original's "Cannot have two hexes together that
    /// have more than 1 altitude difference").
    pub fn paint(&mut self, x: usize, y: usize, paint: Paint) -> Result<(), String> {
        if x >= self.map.width || y >= self.map.height {
            return Err("off the map".into());
        }
        let level = match paint {
            Paint::Level(n) => Some(n.clamp(1, 9)),
            Paint::Water => Some(0),
            _ => None,
        };
        if let Some(z) = level {
            let near = Hex::new(x as i32, y as i32).neighbours();
            if near.iter().any(|&n| self.on_map(n) && (self.map.elevation_at(n.x as usize, n.y as usize) as i32 - z as i32).abs() > 1) {
                return Err("a neighbouring hex is more than one level away".into());
            }
            self.map.elevation[y * self.map.width + x] = z;
            return Ok(());
        }
        let (gx, gy) = Map::hex_cell(x, y);
        let w = self.map.grid_width();
        let cell = &mut self.map.cells[gy as usize * w + gx as usize];
        cell.retain(|c| ![WOODS, ROCKY, ROAD].contains(c));
        match paint {
            Paint::Woods => cell.push(WOODS),
            Paint::Rocky => cell.push(ROCKY),
            Paint::Road => cell.push(ROAD),
            _ => {}
        }
        Ok(())
    }

    /// Put `code` (a stream, a wall) on side `side` of hex `(x, y)`, or
    /// take it off if it's there.
    pub fn toggle_side(&mut self, x: usize, y: usize, side: MapSide, code: u16) {
        let (gx, gy) = Map::hex_cell(x, y);
        let (dx, dy) = side.offset();
        let (cx, cy) = (gx + dx, gy + dy);
        if cx < 0 || cy < 0 || cx as usize >= self.map.grid_width() || cy as usize >= self.map.grid_height() {
            return;
        }
        let w = self.map.grid_width();
        let cell = &mut self.map.cells[cy as usize * w + cx as usize];
        if let Some(k) = cell.iter().position(|&c| c == code) {
            cell.remove(k);
        } else {
            cell.push(code);
        }
    }

    /// Whether side `side` of hex `(x, y)` has `code` (a stream, a wall).
    pub fn has_side(&self, x: usize, y: usize, side: MapSide, code: u16) -> bool {
        let (gx, gy) = Map::hex_cell(x, y);
        let (dx, dy) = side.offset();
        self.map.cell(gx + dx, gy + dy).contains(&code)
    }

    /// The ground of hex `(x, y)`: woods, rough ground, a road, or clear.
    pub fn ground(&self, x: usize, y: usize) -> Paint {
        let (gx, gy) = Map::hex_cell(x, y);
        let cell = self.map.cell(gx, gy);
        [(WOODS, Paint::Woods), (ROCKY, Paint::Rocky), (ROAD, Paint::Road)].into_iter().find(|(c, _)| cell.contains(c)).map_or(Paint::Clear, |(_, p)| p)
    }

    /// Clear hex `(x, y)`'s properties (the original's Clear Hex
    /// Properties): its ground, and the streams and walls on its sides.
    pub fn clear_hex(&mut self, x: usize, y: usize) {
        if x >= self.map.width || y >= self.map.height {
            return;
        }
        let _ = self.paint(x, y, Paint::Clear);
        for side in MapSide::ALL {
            for code in [STREAM, WALL] {
                if self.has_side(x, y, side, code) {
                    self.toggle_side(x, y, side, code);
                }
            }
        }
    }

    /// Resize the map to `width` × `height` hexes, keeping its terrain
    /// where the two overlap. Refused if a unit would fall off it (as the
    /// original's "could not be resized, without removing units"), or an
    /// odd size: the width must be odd (as the original's), and both at
    /// most [`MAX_SIZE`].
    pub fn resize(&mut self, width: usize, height: usize) -> Result<(), String> {
        if width.is_multiple_of(2) || !(1..=MAX_SIZE).contains(&width) || !(1..=MAX_SIZE).contains(&height) {
            return Err(format!("the width must be odd, and both sizes at most {MAX_SIZE}"));
        }
        if self.taken(None).iter().any(|h| h.x as usize >= width || h.y as usize >= height) {
            return Err("the map can't be resized without removing units".into());
        }
        let mut map = Map::blank(&self.map.name, width, height);
        map.hex_id_origin = self.map.hex_id_origin;
        map.unk_b = self.map.unk_b;
        for y in 0..height.min(self.map.height) {
            for x in 0..width.min(self.map.width) {
                map.elevation[y * width + x] = self.map.elevation_at(x, y);
            }
        }
        // (the feature grid, cell by cell where both have it)
        let (gw, gh) = (map.grid_width(), map.grid_height());
        for gy in 0..gh.min(self.map.grid_height()) {
            for gx in 0..gw.min(self.map.grid_width()) {
                map.cells[gy * gw + gx] = self.map.cells[gy * self.map.grid_width() + gx].clone();
            }
        }
        self.map = map;
        Ok(())
    }

    /// Place a copy of `template` (a catalogue unit) on hex `(x, y)`,
    /// fighting for the home side (`home`) or the visitor. A leader starts
    /// a group of his own; any other unit joins `group`, whose leader must
    /// be of its side. It faces the enemy from its side's retreat edge.
    /// Returns its index.
    pub fn place(&mut self, template: &Unit, x: usize, y: usize, home: bool, group: Option<i16>) -> Result<usize, String> {
        let h = Hex::new(x as i32, y as i32);
        if !self.on_map(h) {
            return Err("off the map".into());
        }
        if self.taken(None).contains(&h) {
            return Err("the hex is taken".into());
        }
        let side = if home { self.scn.home } else { self.scn.visitor };
        if side.nation == 255 {
            return Err("choose the side's nation first (Scenario)".into());
        }
        let mut u = template.clone();
        u.layout = Layout::Editor;
        u.nation = side.nation;
        u.art_nation = side.nation;
        u.hex = self.hex_id(x, y);
        // facing the enemy: away from the retreat edge (never straight up
        // or down the map)
        u.facing = (side.retreat + 3) % 8 + 1;
        if matches!(u.facing, 1 | 5) {
            u.facing = u.facing % 8 + 1;
        }
        if two_hex(&u) && !flank(h, u.facing).is_some_and(|s| self.on_map(s) && !self.taken(None).contains(&s)) {
            return Err("a double-sized unit needs its second hex free".into());
        }
        if self.scn.units.iter().any(|o| o.name == u.name && o.nation == u.nation && o.kind == u.kind) {
            return Err(format!("{} is already on the field", u.name));
        }
        if u.kind == 0 {
            let id = self.next_group;
            self.next_group += 1;
            let mut name: String = u.name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
            while self.info().groups.iter().any(|g| g.name == name) {
                name.insert(0, '_');
            }
            self.info().groups.push(Group { id, name: name.clone(), orders: vec![(0, 0, 0); 20], behaviour: 3 });
            self.scn.leaders.push((id as u16, name));
            u.unk_b = id as u16;
        } else {
            let g = group.ok_or("choose a leader's group first")?;
            let leader = self.leader_of(g).ok_or("no such group")?;
            if self.scn.units[leader].nation != u.nation {
                return Err("the unit must be of its group leader's side".into());
            }
            u.unk_b = g as u16;
        }
        self.scn.units.push(u);
        Ok(self.scn.units.len() - 1)
    }

    /// The leader of group `g` (his index).
    pub fn leader_of(&self, g: i16) -> Option<usize> {
        self.scn.units.iter().position(|u| u.kind == 0 && u.unk_b as i16 == g)
    }

    /// Move unit `i` to hex `(x, y)` (its second hex too, if it has one).
    pub fn move_unit(&mut self, i: usize, x: usize, y: usize) -> Result<(), String> {
        let h = Hex::new(x as i32, y as i32);
        let taken = self.taken(Some(i));
        let u = &self.scn.units[i];
        if !self.on_map(h) || taken.contains(&h) {
            return Err("the hex is taken".into());
        }
        if two_hex(u) && !flank(h, u.facing).is_some_and(|s| self.on_map(s) && !taken.contains(&s)) {
            return Err("a double-sized unit needs its second hex free".into());
        }
        let id = self.hex_id(x, y);
        self.scn.units[i].hex = id;
        Ok(())
    }

    /// Turn unit `i` a sixth of a turn (`right`: clockwise).
    pub fn reface(&mut self, i: usize, right: bool) -> Result<(), String> {
        const ORDER: [u8; 6] = [2, 3, 4, 6, 7, 8];
        let u = &self.scn.units[i];
        let k = ORDER.iter().position(|&f| f == u.facing).unwrap_or(1);
        let facing = ORDER[if right { (k + 1) % 6 } else { (k + 5) % 6 }];
        if two_hex(u) {
            let h = self.map.hex_from_id(u.hex).map(|(x, y)| Hex::new(x as i32, y as i32)).ok_or("off the map")?;
            let taken = self.taken(Some(i));
            if !flank(h, facing).is_some_and(|s| self.on_map(s) && !taken.contains(&s)) {
                return Err("the double-sized unit has no room to turn".into());
            }
        }
        self.scn.units[i].facing = facing;
        Ok(())
    }

    /// Take unit `i` off the field (a leader only once his group is empty;
    /// his group goes with him).
    pub fn remove(&mut self, i: usize) -> Result<(), String> {
        let u = &self.scn.units[i];
        if u.kind == 0 {
            let g = u.unk_b;
            if self.scn.units.iter().any(|o| o.kind != 0 && o.unk_b == g) {
                return Err("remove the leader's units first".into());
            }
            self.info().groups.retain(|gr| gr.id as u16 != g);
            self.scn.leaders.retain(|l| l.0 != g);
        }
        self.scn.units.remove(i);
        Ok(())
    }

    /// The unit on hex `(x, y)`, if any.
    pub fn unit_at(&self, x: usize, y: usize) -> Option<usize> {
        let id = self.hex_id(x, y);
        let h = Hex::new(x as i32, y as i32);
        self.scn.units.iter().position(|u| u.hex == id).or_else(|| {
            self.scn
                .units
                .iter()
                .position(|u| two_hex(u) && self.map.hex_from_id(u.hex).and_then(|(ux, uy)| flank(Hex::new(ux as i32, uy as i32), u.facing)) == Some(h))
        })
    }

    /// What must be set before saving (the original's checks): both
    /// nations, retreat edges, rout levels and the turn limit within
    /// 1..=5000, a title.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.scn.title.trim().is_empty() {
            out.push("the scenario needs a title".into());
        }
        for (name, s) in [("home", self.scn.home), ("visitor", self.scn.visitor)] {
            if s.nation as usize >= NATIONS.len() {
                out.push(format!("the {name} side needs a nation"));
            }
            if !matches!(s.retreat, 1 | 3 | 5 | 7) {
                out.push(format!("the {name} side needs a retreat edge"));
            }
            if !(1..=5000).contains(&s.rout_level) {
                out.push(format!("the {name} side's rout level must be 1 to 5000"));
            }
        }
        if let Some((t, _)) = self.scn.editor.as_ref().and_then(|e| e.turn_limit)
            && !(1..=5000).contains(&t)
        {
            out.push("the turn limit must be 1 to 5000".into());
        }
        out
    }

    /// Side `home`'s AI plan, from its groups' orders (the original's
    /// generated plan): each group defends while it has no units of its own
    /// (it commands others) or after heavy losses; then carries out its
    /// orders in turn, each for its turns (a move until its hex is reached);
    /// then attacks or defends as its behaviour says.
    pub fn ai_script(&self, home: bool) -> String {
        const ORDINAL: [&str; 20] = [
            "First",
            "Second",
            "Third",
            "Fourth",
            "Fifth",
            "Sixth",
            "Seventh",
            "Eighth",
            "Ninth",
            "Tenth",
            "Eleventh",
            "Twelfth",
            "Thirteenth",
            "Fourteenth",
            "Fifteenth",
            "Sixteenth",
            "Seventeenth",
            "Eighteenth",
            "Nineteenth",
            "Twentieth",
        ];
        let side = if home { self.scn.home } else { self.scn.visitor };
        let nation = NATIONS.get(side.nation as usize).copied().unwrap_or("Unknown");
        let origin = self.map.hex_id_origin;
        let groups: Vec<&Group> =
            self.scn.editor.iter().flat_map(|e| &e.groups).filter(|g| self.leader_of(g.id).is_some_and(|l| self.scn.units[l].nation == side.nation)).collect();
        // each group's orders in use: (ordinal, order)
        let plans: Vec<(&Group, Vec<(&str, Order)>)> =
            groups.iter().map(|g| (*g, g.orders.iter().enumerate().filter(|(_, o)| o.0 != 0).map(|(k, &o)| (ORDINAL[k], o)).collect())).collect();
        let mut s = format!("' {nation} A/I Plan #1\nAutoGeneratedAI\n{{\n\timport \"basicman.ai\";\n\n");
        let flags: Vec<String> = plans.iter().flat_map(|(g, os)| os.iter().map(move |(o, _)| format!("{}{o}MoveDone", g.name))).collect();
        if !flags.is_empty() {
            s += &format!("\tvar {};\n\n", flags.join(",\n\t\t"));
        }
        for (g, os) in &plans {
            for (o, _) in os {
                s += &format!("\tprocedure Figure{}{o}Move();\n", g.name);
            }
        }
        s += "\n\tConditions\n\t{\n\t\tvar Default, NoUnits, Regroup";
        for (g, os) in &plans {
            for (o, _) in os {
                s += &format!(", {}{o}Move", g.name);
            }
        }
        s += ";\n\n\t\tDefault = True;\n\t\tNoUnits = (GroupListGetSize() == 0);\n\t\tRegroup = FigurePriority(3, 40);\n";
        for (g, os) in &plans {
            for (o, _) in os {
                s += &format!("\t\t{0}{o}Move = Figure{0}{o}Move();\n", g.name);
            }
        }
        s += "\t};\n\n";
        for (g, os) in &plans {
            let mut until = 0u32;
            for (o, (kind, turns, hex)) in os {
                until += *turns as u32;
                let flag = format!("{}{o}MoveDone", g.name);
                s += &format!("\tprocedure Figure{}{o}Move()\n\t{{\n\t\tif ({flag})\n\t\t\treturn False;\n", g.name);
                s += &format!("\t\tif (GetTurnNum() > {until})\n\t\t{{\n\t\t\t{flag} = True;\n\t\t\treturn False;\n\t\t}};\n");
                if *kind == 2 {
                    let at = hex.saturating_sub(origin);
                    s += &format!("\t\tif (PosAcheived({at}, {}))\n\t\t{{\n\t\t\t{flag} = True;\n\t\t\treturn False;\n\t\t}};\n", nation.to_uppercase());
                }
                s += "\t\treturn True;\n\t};\n\n";
            }
        }
        for (g, os) in &plans {
            s += &format!("\tOrders for {}\n\t{{\n\t\tNoUnits CommandOtherUnits();\n\t\tRegroup Defend();\n", g.name);
            for (o, (kind, _, hex)) in os {
                let call = match kind {
                    1 => "Defend()".to_string(),
                    2 => format!("AggressiveGroupMove({})", hex.saturating_sub(origin)),
                    _ => "GroupBerserk()".to_string(),
                };
                s += &format!("\t\t{}{o}Move {call};\n", g.name);
            }
            s += &format!("\t\tDefault {};\n\t}};\n\n", if g.behaviour == 3 { "GroupBerserk()" } else { "Defend()" });
        }
        s + "}\n"
    }

    /// The files to save the scenario as `base` (`<base>.scn`, the map as
    /// `<map file>`, its plans `<base>_H1.AI` and `<base>_V1.AI`): name and
    /// contents. The scenario and map get fresh ids (`guid`, `map_guid`).
    pub fn files(&mut self, base: &str, guid: [u8; 16], map_guid: [u8; 16]) -> Vec<(String, Vec<u8>)> {
        let (h, v) = (format!("{base}_H1.AI"), format!("{base}_V1.AI"));
        self.scn.home_ai = vec![h.clone()];
        self.scn.visitor_ai = vec![v.clone()];
        self.info().guid = guid;
        vec![
            (format!("{base}.scn"), self.scn.write_editor()),
            (self.scn.map_file.clone(), self.map.write(Some(map_guid))),
            (h, self.ai_script(true).into_bytes()),
            (v, self.ai_script(false).into_bytes()),
        ]
    }
}

/// Whether a unit record is double-sized (phalanxes, Hannibal's doubled
/// hoplites...).
pub fn two_hex(u: &Unit) -> bool {
    match u.extra {
        UnitExtra::Combat { c, .. } | UnitExtra::Missile { c, .. } => c != 0,
        UnitExtra::Leader(_) => false,
    }
}

/// Take a game's scenario over as an editor one: each unit's art from the
/// type the game gives it, its kind from its class.
fn convert(scn: &mut Scenario) {
    use gboh_formats::units::{EDITOR_ART, unit_type};
    for u in &mut scn.units {
        let ty = unit_type(u);
        u.artwork = ty.and_then(|t| EDITOR_ART.iter().position(|&e| e == t)).map_or(-1, |k| k as i16);
        u.kind = editor_kind(u);
        u.layout = Layout::Editor;
    }
    scn.units.retain(|u| u.artwork >= 0);
    let groups = scn
        .leaders
        .iter()
        .map(|(id, name)| Group {
            id: *id as i16,
            name: name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect(),
            orders: vec![(0, 0, 0); 20],
            behaviour: 3,
        })
        .collect();
    scn.layout = Layout::Editor;
    scn.editor = Some(EditorInfo { groups, ..Default::default() });
}

/// The editor kind a game unit is written with, by its type's class.
fn editor_kind(u: &Unit) -> u8 {
    use crate::movement::MoveClass::*;
    let Some(ty) = gboh_formats::units::unit_type(u) else { return 6 };
    if gboh_formats::units::hannibal_class(ty) == Some(gboh_formats::units::Class::Leader) || matches!(u.extra, UnitExtra::Leader(_)) {
        return 0;
    }
    match crate::movement::MoveClass::of_type(ty) {
        Some(Phalanx) => 12,
        Some(Legion) if ty >= gboh_formats::units::CAESAR => 17,
        Some(Legion) => 13,
        Some(HeavyInfantry) => 6,
        Some(MediumInfantry) => 8,
        Some(BarbarianInfantry) => 15,
        Some(LightInfantryMG | LightInfantry) => 7,
        Some(Skirmisher) => 10,
        Some(HeavyCavalry) => 2,
        Some(RomanCavalry) => 14,
        Some(BarbarianCavalry) => 16,
        Some(Lancer) => 9,
        Some(LightCavalry) => 3,
        Some(Elephant) => 5,
        Some(Chariot) => 4,
        Some(Oxybeles) => 11,
        None => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leader(name: &str) -> Unit {
        Unit {
            kind: 0,
            name: name.into(),
            nation: 0,
            unk_a: 9,
            unk_b: 0,
            hex: 0,
            facing: 0,
            extra: UnitExtra::Leader([2, 5, 4, 0]),
            arrival: 0,
            elite: 0,
            link: -1,
            layout: Layout::Editor,
            art_nation: 0,
            artwork: 8,
            catalogue: 1,
        }
    }

    fn infantry(name: &str) -> Unit {
        Unit { kind: 6, artwork: 45, extra: UnitExtra::Combat { a: 6, b: 7, c: 0, more: None }, ..leader(name) }
    }

    fn editor() -> Editor {
        let mut e = Editor::new(Map::blank("Test", 15, 15), "TEST.MAP");
        e.scn.title = "Test".into();
        (e.scn.home.nation, e.scn.home.retreat) = (2, 7);
        (e.scn.visitor.nation, e.scn.visitor.retreat) = (1, 3);
        e
    }

    /// Leaders make groups, units join them on their side and face the
    /// enemy; taken hexes and duplicates are refused; leaders stay while
    /// their units do.
    #[test]
    fn places_units_by_the_rules() {
        let mut e = editor();
        assert!(e.place(&infantry("Hypaspists"), 3, 3, true, None).is_err(), "no group yet");
        let l = e.place(&leader("Alexander"), 3, 3, true, None).unwrap();
        let g = e.scn.units[l].unk_b as i16;
        assert_eq!(e.scn.units[l].facing, 3, "faces away from a west edge");
        assert!(e.place(&infantry("Hypaspists"), 3, 3, true, Some(g)).is_err(), "taken");
        let u = e.place(&infantry("Hypaspists"), 4, 3, true, Some(g)).unwrap();
        assert!(e.place(&infantry("Hypaspists"), 5, 3, true, Some(g)).is_err(), "duplicate");
        assert!(e.place(&infantry("Immortals"), 9, 9, false, Some(g)).is_err(), "group of the other side");
        assert!(e.remove(l).is_err(), "his units first");
        e.remove(u).unwrap();
        e.remove(l).unwrap();
        assert!(e.scn.units.is_empty() && e.scn.editor.as_ref().unwrap().groups.is_empty());
    }

    /// Levels are painted at most one apart from their neighbours.
    #[test]
    fn paints_terrain() {
        let mut e = editor();
        e.paint(5, 5, Paint::Level(2)).unwrap();
        assert!(e.paint(5, 6, Paint::Level(4)).is_err());
        e.paint(5, 5, Paint::Woods).unwrap();
        assert_eq!(e.map.hex_features(5, 5), &[1]);
        e.paint(5, 5, Paint::Road).unwrap();
        assert_eq!(e.map.hex_features(5, 5), &[12]);
    }

    /// What it saves reads back, and its plans parse.
    #[test]
    fn hex_properties_and_resizing() {
        let mut e = editor();
        e.paint(5, 5, Paint::Level(2)).unwrap();
        e.paint(5, 5, Paint::Rocky).unwrap();
        e.toggle_side(5, 5, MapSide::NE, STREAM);
        assert_eq!(e.ground(5, 5), Paint::Rocky);
        assert!(e.has_side(5, 5, MapSide::NE, STREAM) && !e.has_side(5, 5, MapSide::N, STREAM));
        e.resize(9, 8).unwrap();
        assert_eq!((e.map.width, e.map.height, e.map.elevation_at(5, 5)), (9, 8, 2));
        assert!(e.has_side(5, 5, MapSide::NE, STREAM) && e.ground(5, 5) == Paint::Rocky);
        e.clear_hex(5, 5);
        assert!(!e.has_side(5, 5, MapSide::NE, STREAM) && e.ground(5, 5) == Paint::Clear);
        assert!(e.resize(8, 8).is_err(), "an even width");
        e.place(&leader("Alexander"), 7, 7, true, None).unwrap();
        assert!(e.resize(7, 7).is_err(), "a unit would fall off");
        Map::parse(&e.map.write(None)).unwrap();
    }
    #[test]
    fn saves_and_reads_back() {
        let mut e = editor();
        let l = e.place(&leader("Alexander"), 3, 3, true, None).unwrap();
        let g = e.scn.units[l].unk_b as i16;
        e.place(&infantry("Hypaspists"), 4, 3, true, Some(g)).unwrap();
        e.info().groups[0].orders[0] = (2, 3, e.hex_id(8, 3));
        e.place(&leader("Darius"), 10, 10, false, None).unwrap();
        let files = e.files("TEST", [1; 16], [2; 16]);
        let scn = Scenario::parse(&files[0].1).unwrap();
        assert_eq!(scn.units.len(), 3);
        assert_eq!(scn.editor.unwrap().groups[0].orders[0], (2, 3, e.hex_id(8, 3)));
        assert_eq!(Map::parse(&files[1].1).unwrap().width, 15);
        for (name, data) in &files[2..] {
            let text = String::from_utf8(data.clone()).unwrap();
            let basic = "procedure CommandOtherUnits() { return 0; }; procedure Defend() { return 0; }; procedure GroupBerserk() { return 0; };
                procedure FigurePriority(a, b) { return 0; }; procedure PosAcheived(h, n) { return 0; }; procedure AggressiveGroupMove(h) { return 0; };";
            let mut load = |f: &str| Some(if f.eq_ignore_ascii_case("basicman.ai") { basic.to_string() } else { text.clone() });
            crate::ai::Script::load(name, &mut load).unwrap_or_else(|e| panic!("{name}: {e}\n{text}"));
        }
    }
}
