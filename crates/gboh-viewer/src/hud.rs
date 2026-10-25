//! The battle window's toolbar and status bar.
//!
//! The toolbar is a strip of 24 × 22 buttons (`BUTTONS1`–`3.EIF`, each with
//! four cels: up, down, disabled, disabled down) on marble, in the order and
//! at the places of the original's (its toolbar setup, `FUN_0051eab8`; places
//! measured on the original: button centres at x 17, 45, 70 … 502 in a 30
//! pixel strip). The status bar is the `Alex Status Bar` art (`ALEXSB.EIF`:
//! end caps, a middle piece that tiles and a divider) in three panes. The
//! overview map is the battle's `OVERVIEW.EIF` scroll (cel 0; cel 1 is
//! the reversed view, cel 2 the bare map, which lies 30 × 26 pixels into
//! the scroll) at the top right, with the units as dots and the part of the
//! battlefield in view outlined.

use crate::ui::{Font, cels, eif, first};
use gboh_render::{Canvas, Sprite};
use std::path::Path;

/// A toolbar button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    OpenMap,
    FinishLeader,
    CenterOnLeader,
    RotateLeft,
    RotateRight,
    AboutFace,
    MissileVolley,
    RestoreCohesion,
    Rally,
    UnitDetails,
    Overview,
    HexGrid,
    ReverseView,
    ZoomIn,
    ZoomNormal,
    ZoomOut,
    TileVertical,
    TileHorizontal,
    Cascade,
    Undo,
    /// Hannibal's: close the game, the leader's group move, manipular line
    /// extension, a unit's backward movement.
    CloseGame,
    GroupMove,
    LineExtension,
    BackwardMovement,
    /// Caesar's: the group attack (in the place of the line extension).
    GroupAttack,
}

/// The buttons: which, their art (file and strip) and the x of their
/// centre.
const TOOLBAR: [(Tool, &str, &str, i32); 20] = [
    (Tool::OpenMap, "BUTTONS2.EIF", "FileOpen", 17),
    (Tool::FinishLeader, "BUTTONS3.EIF", "NextPhase", 45),
    (Tool::CenterOnLeader, "BUTTONS2.EIF", "CenterOnLeader", 70),
    (Tool::RotateLeft, "BUTTONS1.EIF", "RotateCounterClockwise", 97),
    (Tool::RotateRight, "BUTTONS1.EIF", "RotateClockwise", 122),
    (Tool::AboutFace, "BUTTONS3.EIF", "AboutFace", 147),
    (Tool::MissileVolley, "BUTTONS1.EIF", "MissleVolley", 171),
    (Tool::RestoreCohesion, "BUTTONS1.EIF", "RestoreCohesion", 195),
    (Tool::Rally, "BUTTONS1.EIF", "Rally", 219),
    (Tool::UnitDetails, "BUTTONS3.EIF", "ViewUnitDetails", 243),
    (Tool::Overview, "BUTTONS2.EIF", "HideOverview", 272),
    (Tool::HexGrid, "BUTTONS3.EIF", "ShowHexGridOn", 296),
    (Tool::ReverseView, "BUTTONS3.EIF", "Rotate180", 320),
    (Tool::ZoomIn, "BUTTONS2.EIF", "ZoomIn", 348),
    (Tool::ZoomNormal, "BUTTONS2.EIF", "ZoomOut", 373),
    (Tool::ZoomOut, "BUTTONS2.EIF", "ZoomOut2", 397),
    (Tool::TileVertical, "BUTTONS2.EIF", "TileVertical", 425),
    (Tool::TileHorizontal, "BUTTONS2.EIF", "TileHorizontal", 449),
    (Tool::Cascade, "BUTTONS2.EIF", "Cascade", 475),
    (Tool::Undo, "BUTTONS3.EIF", "Undo", 502),
];

/// Where each button's texts lie in `alex.exe` (version 1.3; the
/// toolbar's set-up, `FUN_0051eab8`, hands them to each button): the art
/// cel's name (checked, to know the executable is that version), the short
/// name the tooltip shows and the line the status bar shows. For the
/// overview and the hex grid, the texts for the other state follow.
const TIPS: [(Tool, u32, u32, u32); 20] = [
    (Tool::OpenMap, 0x6216e0, 0x6216f9, 0x6216e9),
    (Tool::FinishLeader, 0x62171c, 0x62175d, 0x621739),
    (Tool::CenterOnLeader, 0x62178a, 0x6217d5, 0x6217ae),
    (Tool::RotateLeft, 0x621808, 0x621855, 0x62182a),
    (Tool::RotateRight, 0x621897, 0x6218d6, 0x6218b3),
    (Tool::AboutFace, 0x621909, 0x621932, 0x621921),
    (Tool::MissileVolley, 0x621959, 0x62199d, 0x621973),
    (Tool::RestoreCohesion, 0x6219cc, 0x621a17, 0x6219ec),
    (Tool::Rally, 0x621a48, 0x621a70, 0x621a58),
    (Tool::UnitDetails, 0x621a97, 0x621ad7, 0x621ab7),
    (Tool::Overview, 0x621afd, 0x621b1c, 0x621b0a),
    (Tool::HexGrid, 0x621b80, 0x621bb3, 0x621b9e),
    (Tool::ReverseView, 0x621c22, 0x621c47, 0x621c2c),
    (Tool::ZoomIn, 0x621c72, 0x621c89, 0x621c79),
    (Tool::ZoomNormal, 0x621caa, 0x621cd1, 0x621cb2),
    (Tool::ZoomOut, 0x621cf2, 0x621d0c, 0x621cfb),
    (Tool::TileVertical, 0x621d33, 0x621d5c, 0x621d40),
    (Tool::TileHorizontal, 0x621d89, 0x621db6, 0x621d98),
    (Tool::Cascade, 0x621de7, 0x621e03, 0x621def),
    (Tool::Undo, 0x621e24, 0x621e43, 0x621e29),
];
/// The overview's "show" and the hex grid's "off" texts.
const SHOW_OVERVIEW: (u32, u32) = (0x621b64, 0x621b52);
const GRID_OFF: (u32, u32) = (0x621c0b, 0x621bf5);

/// Hannibal's toolbar (its set-up builds it in this order, separators
/// between the groups; 🟡 the x positions follow Alexander's spacing).
const HANNIBAL_TOOLBAR: [(Tool, &str, &str, i32); 24] = [
    (Tool::CloseGame, "BUTTONS3.EIF", "GameClose", 17),
    (Tool::FinishLeader, "BUTTONS3.EIF", "NextPhase", 45),
    (Tool::CenterOnLeader, "BUTTONS2.EIF", "CenterOnLeader", 69),
    (Tool::GroupMove, "BUTTONS3.EIF", "GroupMove", 93),
    (Tool::LineExtension, "BUTTONS3.EIF", "MLE", 117),
    (Tool::RotateLeft, "BUTTONS1.EIF", "RotateCounterClockwise", 145),
    (Tool::RotateRight, "BUTTONS1.EIF", "RotateClockwise", 169),
    (Tool::AboutFace, "BUTTONS3.EIF", "AboutFace", 193),
    (Tool::BackwardMovement, "BUTTONS3.EIF", "BackwardMovement", 217),
    (Tool::MissileVolley, "BUTTONS1.EIF", "MissleVolley", 241),
    (Tool::RestoreCohesion, "BUTTONS1.EIF", "RestoreCohesion", 265),
    (Tool::Rally, "BUTTONS1.EIF", "Rally", 289),
    (Tool::UnitDetails, "BUTTONS3.EIF", "ViewUnitDetails", 313),
    (Tool::OpenMap, "BUTTONS2.EIF", "MapOpen", 341),
    (Tool::Overview, "BUTTONS2.EIF", "HideOverview", 365),
    (Tool::HexGrid, "BUTTONS3.EIF", "ShowHexGridOn", 389),
    (Tool::ReverseView, "BUTTONS3.EIF", "Rotate180", 413),
    (Tool::ZoomIn, "BUTTONS2.EIF", "ZoomIn", 441),
    (Tool::ZoomNormal, "BUTTONS2.EIF", "ZoomOut", 465),
    (Tool::ZoomOut, "BUTTONS2.EIF", "ZoomOut2", 489),
    (Tool::TileVertical, "BUTTONS2.EIF", "TileVertical", 517),
    (Tool::TileHorizontal, "BUTTONS2.EIF", "TileHorizontal", 541),
    (Tool::Cascade, "BUTTONS2.EIF", "Cascade", 565),
    (Tool::Undo, "BUTTONS3.EIF", "Undo", 593),
];

/// Where Hannibal's buttons' texts lie in `hannibal.exe` (as [`TIPS`]).
const HANNIBAL_TIPS: [(Tool, u32, u32, u32); 24] = [
    (Tool::CloseGame, 0x63043a, 0x630477, 0x630444),
    (Tool::FinishLeader, 0x63049e, 0x6304d3, 0x6304a8),
    (Tool::CenterOnLeader, 0x630500, 0x630535, 0x63050f),
    (Tool::GroupMove, 0x630568, 0x6305a6, 0x630572),
    (Tool::LineExtension, 0x6305cd, 0x6305cd, 0x6305d1),
    (Tool::RotateLeft, 0x63060c, 0x63064e, 0x630623),
    (Tool::RotateRight, 0x630690, 0x6306c3, 0x6306a0),
    (Tool::AboutFace, 0x6306f6, 0x630711, 0x630700),
    (Tool::BackwardMovement, 0x630738, 0x630771, 0x630749),
    (Tool::MissileVolley, 0x6307a6, 0x6307dd, 0x6307b3),
    (Tool::RestoreCohesion, 0x63080c, 0x630847, 0x63081c),
    (Tool::Rally, 0x630878, 0x630896, 0x63087e),
    (Tool::UnitDetails, 0x6308bd, 0x6308ed, 0x6308cd),
    (Tool::OpenMap, 0x630913, 0x63092b, 0x63091b),
    (Tool::Overview, 0x63094e, 0x63096d, 0x63095b),
    (Tool::HexGrid, 0x6309d1, 0x6309f4, 0x6309df),
    (Tool::ReverseView, 0x630a52, 0x630a77, 0x630a5c),
    (Tool::ZoomIn, 0x630aa2, 0x630ab9, 0x630aa9),
    (Tool::ZoomNormal, 0x630ada, 0x630b01, 0x630ae2),
    (Tool::ZoomOut, 0x630b22, 0x630b3c, 0x630b2b),
    (Tool::TileVertical, 0x630b63, 0x630b8c, 0x630b70),
    (Tool::TileHorizontal, 0x630bb9, 0x630be6, 0x630bc8),
    (Tool::Cascade, 0x630c17, 0x630c33, 0x630c1f),
    (Tool::Undo, 0x630c54, 0x630c54, 0x630c59),
];
const HANNIBAL_SHOW_OVERVIEW: (u32, u32) = (0x6309b5, 0x6309a3);
const HANNIBAL_GRID_OFF: (u32, u32) = (0x630a3b, 0x630a25);

/// Caesar's toolbar: Hannibal's, with the group attack for the line
/// extension; its own strips (🟡 places as Hannibal's).
const CAESAR_TOOLBAR: [(Tool, &str, &str, i32); 24] = [
    (Tool::CloseGame, "BUTTONS1.EIF", "CloseBattle", 17),
    (Tool::FinishLeader, "BUTTONS1.EIF", "FinishLeader", 45),
    (Tool::CenterOnLeader, "BUTTONS1.EIF", "CenterOnLeader", 69),
    (Tool::GroupMove, "BUTTONS1.EIF", "GroupMove", 93),
    (Tool::GroupAttack, "BUTTONS1.EIF", "GroupAttack", 117),
    (Tool::RotateLeft, "BUTTONS1.EIF", "RotateLeft", 145),
    (Tool::RotateRight, "BUTTONS1.EIF", "RotateRight", 169),
    (Tool::AboutFace, "BUTTONS1.EIF", "AboutFace", 193),
    (Tool::BackwardMovement, "BUTTONS1.EIF", "BackwardMovement", 217),
    (Tool::MissileVolley, "BUTTONS1.EIF", "MissileVolley", 241),
    (Tool::RestoreCohesion, "BUTTONS2.EIF", "RestoreCohesion", 265),
    (Tool::Rally, "BUTTONS2.EIF", "Rally", 289),
    (Tool::UnitDetails, "BUTTONS2.EIF", "ViewDetails", 313),
    (Tool::OpenMap, "BUTTONS2.EIF", "OpenMap", 341),
    (Tool::Overview, "BUTTONS2.EIF", "HideOverview", 365),
    (Tool::HexGrid, "BUTTONS2.EIF", "ShowHexGrid", 389),
    (Tool::ReverseView, "BUTTONS3.EIF", "ReverseView", 413),
    (Tool::ZoomIn, "BUTTONS3.EIF", "ZoomIn", 441),
    (Tool::ZoomNormal, "BUTTONS3.EIF", "Normal", 465),
    (Tool::ZoomOut, "BUTTONS3.EIF", "ZoomOut", 489),
    (Tool::TileVertical, "BUTTONS3.EIF", "TileVertical", 517),
    (Tool::TileHorizontal, "BUTTONS3.EIF", "TileHorizontal", 541),
    (Tool::Cascade, "BUTTONS3.EIF", "Cascade", 565),
    (Tool::Undo, "BUTTONS3.EIF", "Undo", 593),
];

/// Where Caesar's buttons' texts lie in `caesar.exe` (as [`TIPS`]).
const CAESAR_TIPS: [(Tool, u32, u32, u32); 24] = [
    (Tool::CloseGame, 0x62ff16, 0x62ff57, 0x62ff22),
    (Tool::FinishLeader, 0x62ff80, 0x62ffb8, 0x62ff8d),
    (Tool::CenterOnLeader, 0x62ffe5, 0x63001a, 0x62fff4),
    (Tool::GroupMove, 0x63004d, 0x63008b, 0x630057),
    (Tool::GroupAttack, 0x6300b2, 0x6300f5, 0x6300be),
    (Tool::RotateLeft, 0x630120, 0x630156, 0x63012b),
    (Tool::RotateRight, 0x630198, 0x6301c7, 0x6301a4),
    (Tool::AboutFace, 0x6301fa, 0x630215, 0x630204),
    (Tool::BackwardMovement, 0x63023c, 0x630275, 0x63024d),
    (Tool::MissileVolley, 0x6302aa, 0x6302e2, 0x6302b8),
    (Tool::RestoreCohesion, 0x630311, 0x63034c, 0x630321),
    (Tool::Rally, 0x63037d, 0x63039b, 0x630383),
    (Tool::UnitDetails, 0x6303c2, 0x6303ee, 0x6303ce),
    (Tool::OpenMap, 0x630419, 0x630431, 0x630421),
    (Tool::Overview, 0x630454, 0x630473, 0x630461),
    (Tool::HexGrid, 0x6304d7, 0x6304f8, 0x6304e3),
    (Tool::ReverseView, 0x630553, 0x63057a, 0x63055f),
    (Tool::ZoomIn, 0x6305a5, 0x6305bc, 0x6305ac),
    (Tool::ZoomNormal, 0x6305dd, 0x630603, 0x6305e4),
    (Tool::ZoomOut, 0x630622, 0x63063b, 0x63062a),
    (Tool::TileVertical, 0x63065e, 0x630687, 0x63066b),
    (Tool::TileHorizontal, 0x6306b4, 0x6306e1, 0x6306c3),
    (Tool::Cascade, 0x630712, 0x63072e, 0x63071a),
    (Tool::Undo, 0x63074f, 0x63076e, 0x630754),
];
const CAESAR_SHOW_OVERVIEW: (u32, u32) = (0x6304bb, 0x6304a9);
const CAESAR_GRID_OFF: (u32, u32) = (0x63053c, 0x630526);

/// A toolbar: its buttons with their art and places, and where their texts
/// lie in the executable (with the overview's and grid's other ones).
#[derive(Clone, Copy)]
struct Toolbar {
    buttons: &'static [(Tool, &'static str, &'static str, i32)],
    tips: &'static [(Tool, u32, u32, u32)],
    other: [(u32, u32); 2],
}

const ALEXANDER_TOOLBAR: Toolbar = Toolbar { buttons: &TOOLBAR, tips: &TIPS, other: [SHOW_OVERVIEW, GRID_OFF] };
const HANNIBAL_TOOLBAR_SET: Toolbar = Toolbar { buttons: &HANNIBAL_TOOLBAR, tips: &HANNIBAL_TIPS, other: [HANNIBAL_SHOW_OVERVIEW, HANNIBAL_GRID_OFF] };
const CAESAR_TOOLBAR_SET: Toolbar = Toolbar { buttons: &CAESAR_TOOLBAR, tips: &CAESAR_TIPS, other: [CAESAR_SHOW_OVERVIEW, CAESAR_GRID_OFF] };

/// Heights of the toolbar and the status bar.
pub const TOOLBAR_HEIGHT: i32 = 30;
pub const STATUS_HEIGHT: i32 = 50;
/// Where the toolbar starts: under the menu bar.
pub const TOOLBAR_TOP: i32 = crate::menu::MENU_HEIGHT;
/// The bottom of the bars at the top of the window.
pub const TOP: i32 = TOOLBAR_TOP + TOOLBAR_HEIGHT;

pub struct Hud {
    /// The game's toolbar.
    toolbar: Toolbar,
    /// Where the left button is held down (window pixels), if it is: a
    /// button under it is drawn pressed.
    pub pressed: std::cell::Cell<Option<(i32, i32)>>,
    /// A tooltip to show, and where (window pixels).
    pub tooltip: std::cell::RefCell<Option<(String, (i32, i32))>>,
    /// The buttons' texts from the executable: short name and status line,
    /// and the overview's and grid's other ones.
    tips: Vec<(Tool, String, String)>,
    other_tips: [(String, String); 2],
    font: Font,
    small: Font,
    marble: Option<Sprite>,
    /// The slider art (`SLIDER.EIF`): thumb, track left, middle and right.
    slider: Vec<Sprite>,
    buttons: Vec<Vec<Sprite>>,
    /// Left cap, middle, right cap, divider.
    bar: Vec<Sprite>,
    /// Each battle's overview scroll and bare map, by its name.
    overviews: Vec<(String, Vec<Sprite>)>,
    /// The unit details window: its art, layout and button art, and the
    /// executable's strings.
    details_art: Option<Sprite>,
    details_dialog: Option<gboh_formats::pe::Dialog>,
    button: Vec<Sprite>,
    big_button: Vec<Sprite>,
    strings: Option<gboh_formats::pe::Resources>,
    /// The combat results dialogs' art and layouts.
    results_art: Option<Sprite>,
    overall_art: Option<Sprite>,
    results_dialog: Option<gboh_formats::pe::Dialog>,
    overall_dialog: Option<gboh_formats::pe::Dialog>,
    /// The game overview's art (the `End of Game` bitmap), layout and the
    /// chiselled letters (`CHISTEXT.EIF`: A–Z, then digits).
    overview_art: Option<Sprite>,
    overview_dialog: Option<gboh_formats::pe::Dialog>,
    chiselled: Vec<Sprite>,
    /// The end of game pictures (`EOGDLG.EIF`, by title) and layout.
    endings: Vec<(String, Sprite)>,
    end_dialog: Option<gboh_formats::pe::Dialog>,
    /// The army details' art and layout.
    army_art: Option<Sprite>,
    army_dialog: Option<gboh_formats::pe::Dialog>,
    /// The palette's remap tables (0 darker, 2 brighter), for engraving.
    remap: [[u8; 256]; 5],
    /// The palette's colour nearest Windows' tooltip yellow.
    pale_yellow: u8,
}

/// One line of the army details: a unit (TQ and hits) or a leader (none),
/// and its rout points if it is out of the battle.
pub struct ArmyRow {
    pub name: String,
    pub tq: Option<u16>,
    pub hits: Option<i32>,
    pub lost: Option<u16>,
}

/// What the army details show for one army.
pub struct ArmyDetails {
    pub nation: u8,
    pub level: u16,
    /// TQ of the army's routing units.
    pub routing_tq: u16,
    /// Rout points of the units out of the battle.
    pub eliminated: u16,
    pub rows: Vec<ArmyRow>,
    /// The first line shown.
    pub scroll: usize,
}

/// Lines the army details' list shows at once.
pub const ARMY_LINES: usize = 10;

/// A dialog drawn from its template (`name` in `alex.exe`): its static texts
/// and edit boxes by control id where they are filled in at run time (id 0:
/// the caption), the check boxes ticked, the list box's lines with the one
/// chosen, and the edit box being typed in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plain {
    pub name: &'static str,
    pub texts: Vec<(u16, String)>,
    pub checked: Vec<u16>,
    pub list: Vec<String>,
    pub selected: Option<usize>,
    pub focus: Option<u16>,
    /// Sliders the dialog adds in code (the preferences' volumes): left and
    /// top in dialog units, and the value out of 10000.
    pub sliders: Vec<(i16, i16, u16)>,
}

/// A slider's full value.
pub const SLIDER_MAX: u16 = 10000;

/// Height of a list box line.
const LIST_LINE: i32 = 13;

/// Height of a dialog's caption bar.
const CAPTION: i32 = 18;

/// A white box with a dark frame.
fn fill_framed(canvas: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32)) {
    for py in y..y + h {
        for px in x..x + w {
            if px >= 0 && py >= 0 && (px as usize) < canvas.width && (py as usize) < canvas.height {
                canvas.pixels[py as usize * canvas.width + px as usize] = 255;
            }
        }
    }
    frame_rect(canvas, (x, y, w, h));
}

/// A one-pixel dark frame.
fn frame_rect(canvas: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32)) {
    let mut put = |px: i32, py: i32| {
        if px >= 0 && py >= 0 && (px as usize) < canvas.width && (py as usize) < canvas.height {
            canvas.pixels[py as usize * canvas.width + px as usize] = 248;
        }
    };
    for i in x..x + w {
        put(i, y);
        put(i, y + h - 1);
    }
    for j in y..y + h {
        put(x, j);
        put(x + w - 1, j);
    }
}

/// A window of text to read: a help topic, the credits, a battle's
/// history or victory conditions. Paragraphs may lead to other topics (the
/// help's contents), and the text scrolls a line at a time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reader {
    pub title: String,
    pub paragraphs: Vec<String>,
    /// The topic each paragraph opens when clicked, if any.
    pub links: Vec<Option<usize>>,
    /// The first line shown.
    pub scroll: usize,
    /// The buttons along the bottom: their captions (the first closes).
    pub buttons: Vec<String>,
}

/// What a click on the reader meets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderClick {
    /// A button, by its place along the bottom.
    Button(usize),
    /// A paragraph's link.
    Link(usize),
}

/// The reader's size, at most.
const READER: (i32, i32) = (560, 420);

/// How a battle ended for the player: a nation byte and whether it won.
pub type Ending = (u8, bool);

/// A point or size on the window, in pixels.
type Point = (i32, i32);

/// A rectangle on the window: left, top, width, height.
type Rect = (i32, i32, i32, i32);

/// What the game overview shows for the two armies (home, visitor):
/// nation bytes, rout levels and rout points lost.
pub struct GameOverview {
    pub nations: [u8; 2],
    pub levels: [u16; 2],
    pub lost: [u16; 2],
}

/// A unit's line in the combat results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRow {
    pub name: String,
    /// Hits taken in this combat, in all, and still to take before routing.
    pub current: i32,
    pub total: i32,
    pub to_rout: i32,
    pub routed: bool,
    pub eliminated: bool,
}

/// The combat results being shown: how many battles, the one picked, and
/// its details if open (attackers, defenders).
pub struct Results {
    pub battles: usize,
    pub selected: Option<usize>,
    pub open: Option<(Vec<ResultRow>, Vec<ResultRow>)>,
}

/// What a click on the combat results did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultsClick {
    Battle(usize),
    Close,
    CloseDetails,
}

/// Dialog units to pixels.
fn du((x, y, w, h): (i16, i16, i16, i16)) -> (i32, i32, i32, i32) {
    (x as i32 * 7 / 4, y as i32 * 13 / 8, w as i32 * 7 / 4, h as i32 * 13 / 8)
}

/// What the unit details window shows (`ViewUnitDlg`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub name: String,
    /// Moved, fired or done this turn.
    pub used: bool,
    pub routing: bool,
    pub tq: u16,
    pub hits: i32,
    pub mp_allowance: i32,
    pub mp_left: i32,
    pub size: u16,
    /// Missiles: the type's string id (64 archer, 65 slinger, 66 javelin,
    /// 67 bolts), range, supply and missiles left.
    pub missile: Option<(u16, i32, u16, u16)>,
}

/// What the overview map shows: the battle (its name in `OVERVIEW.EIF`),
/// units as dots (where on the battlefield, 0 to 1 across and down, and
/// the palette colour), and the part in view (left, top, right, bottom).
pub struct Overview<'a> {
    pub battle: &'a str,
    pub dots: Vec<(f32, f32, u8)>,
    pub view: [f32; 4],
    /// The reversed view: the scroll with the map turned (cel 1).
    pub reversed: bool,
}

/// Where the bare map lies in the overview scroll.
const MAP_IN_SCROLL: (i32, i32) = (30, 26);

impl Hud {
    pub fn new(data_dir: &Path) -> Self {
        let files: Vec<_> = ["BUTTONS1.EIF", "BUTTONS2.EIF", "BUTTONS3.EIF"].iter().map(|f| (*f, eif(data_dir, f))).collect();
        let toolbar = match gboh_formats::game::Game::detect(data_dir) {
            Some(gboh_formats::game::Game::Hannibal) => HANNIBAL_TOOLBAR_SET,
            Some(gboh_formats::game::Game::Caesar) => CAESAR_TOOLBAR_SET,
            _ => ALEXANDER_TOOLBAR,
        };
        let buttons = toolbar
            .buttons
            .iter()
            .map(|&(_, file, strip, _)| {
                files.iter().find(|(f, _)| *f == file).map_or_else(Vec::new, |(_, e)| {
                    // up, down, then disabled, by the cels' names where they
                    // have them (Caesar's files put the down cel first)
                    let mut art = cels(e, strip);
                    if let Some(st) = e.as_ref().and_then(|e| e.strips.iter().find(|s| s.name == strip)) {
                        let at = |name: &str| st.cels.iter().position(|c| c.name == name);
                        if let (Some(up), Some(down)) = (at("UpEnabled"), at("DownEnabled")) {
                            let mut order = vec![up, down];
                            order.extend(at("UpDisabled"));
                            order.extend(at("DownDisabled"));
                            art = order.into_iter().filter_map(|k| art.get(k).cloned()).collect();
                        }
                    }
                    art
                })
            })
            .collect();
        Self {
            toolbar,
            pressed: std::cell::Cell::new(None),
            tooltip: std::cell::RefCell::new(None),
            tips: Vec::new(),
            other_tips: Default::default(),
            // (Hannibal's glyphs are pale in its palette: drawn in its
            // darkest colour, as the small font is)
            font: match Font::small(data_dir).ink() {
                Some(ink) if gboh_formats::game::Game::detect(data_dir).is_some_and(|g| g != gboh_formats::game::Game::Alexander) => {
                    Font::load(data_dir).in_ink(ink)
                }
                _ => Font::load(data_dir),
            },
            small: Font::small(data_dir),
            marble: first(&eif(data_dir, "BACKGND.EIF"), "Blocks"),
            slider: cels(&eif(data_dir, "SLIDER.EIF"), "Horizontal"),
            buttons,
            // (Hannibal's and Caesar's: `STATBAR.EIF`)
            bar: Some(cels(&eif(data_dir, "ALEXSB.EIF"), "sbArt")).filter(|c| !c.is_empty()).unwrap_or_else(|| cels(&eif(data_dir, "STATBAR.EIF"), "sbArt")),
            details_art: first(&eif(data_dir, "DIALOG.EIF"), "Unit Overview"),
            results_art: first(&eif(data_dir, "DIALOG.EIF"), "Combat Results"),
            overall_art: first(&eif(data_dir, "DIALOG.EIF"), "Overall Combat Results"),
            results_dialog: None,
            overall_dialog: None,
            overview_art: first(&eif(data_dir, "DIALOG.EIF"), "End of Game"),
            overview_dialog: None,
            chiselled: cels(&eif(data_dir, "CHISTEXT.EIF"), "Alpha"),
            army_art: first(&eif(data_dir, "DIALOG.EIF"), "Army Details"),
            endings: eif(data_dir, "EOGDLG.EIF")
                .map(|e| {
                    let names: Vec<String> = e.strips.iter().map(|s| s.name.clone()).collect();
                    names.into_iter().filter_map(|n| Some((n.clone(), first(&Some(e.clone()), &n)?))).collect()
                })
                .unwrap_or_default(),
            end_dialog: None,
            army_dialog: None,
            remap: crate::palette_path(data_dir)
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|d| gboh_formats::pal::Palette::parse(&d).ok())
                .and_then(|p| p.remap.try_into().ok())
                .unwrap_or([std::array::from_fn(|i| i as u8); 5]),
            pale_yellow: crate::palette_path(data_dir)
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|d| gboh_formats::pal::Palette::parse(&d).ok())
                .and_then(|p| {
                    (1..=255u8).min_by_key(|&i| {
                        let [r, g, b] = p.colors[i as usize];
                        (255 - r as i32).pow(2) + (255 - g as i32).pow(2) + (225 - b as i32).pow(2)
                    })
                })
                .unwrap_or(255),
            button: cels(&eif(data_dir, "DLGBTN.EIF"), "Pushbutton"),
            big_button: cels(&eif(data_dir, "DLGBTN.EIF"), "Big Pushbutton"),
            details_dialog: None,
            strings: None,
            overviews: eif(data_dir, "OVERVIEW.EIF")
                .map(|e| {
                    e.strips
                        .iter()
                        .map(|s| {
                            let mut cels = gboh_render::strip(&e, &s.name).unwrap_or_default();
                            // the scroll's shadow (index 255) would darken the
                            // battlefield; the overlay cannot, so it is left out
                            for c in cels.iter_mut() {
                                c.pixels.iter_mut().filter(|p| **p == 255).for_each(|p| *p = 0);
                            }
                            (s.name.clone(), cels)
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
        .with_exe(data_dir)
    }

    /// The executable's strings and the details window's layout (from
    /// `alex.exe` beside `DATA`).
    fn with_exe(mut self, data_dir: &Path) -> Self {
        let exe = crate::ui::exe_path(data_dir).and_then(|p| std::fs::read(p).ok());
        self.strings = exe.and_then(|d| gboh_formats::pe::Resources::parse(&d).ok());
        self.details_dialog = self.strings.as_ref().and_then(|r| r.dialog("VIEWUNIT"));
        self.results_dialog = self.strings.as_ref().and_then(|r| r.dialog("SHOWCOMBATRESULTS"));
        self.overall_dialog = self.strings.as_ref().and_then(|r| r.dialog("OVERALLCOMBATRESULTS"));
        self.overview_dialog = self.strings.as_ref().and_then(|r| r.dialog("GAMEOVERVIEWDLG"));
        self.army_dialog = self.strings.as_ref().and_then(|r| r.dialog("ARMYDETAILSDLG"));
        self.end_dialog = self.strings.as_ref().and_then(|r| r.dialog("ENDOFGAMEDLG"));
        // the toolbar's texts, if the executable is the version they are
        // known in
        if let Some(d) = crate::ui::exe_path(data_dir).and_then(|p| std::fs::read(p).ok()) {
            let at = |va: u32| gboh_formats::pe::string_at(&d, va).unwrap_or_default();
            let tb = self.toolbar;
            let known = tb.buttons.iter().zip(tb.tips).all(|(&(tool, _, cel, _), &(t, name, _, _))| tool == t && at(name) == cel);
            if known {
                self.tips = tb.tips.iter().map(|&(t, _, short, long)| (t, at(short), at(long))).collect();
                let [show, off] = tb.other;
                self.other_tips = [(at(show.0), at(show.1)), (at(off.0), at(off.1))];
            }
        }
        self
    }

    /// A nation's name (strings 31–36 run Greek, Macedonian, Persian,
    /// Danubian, Indian, Scythian; nation bytes Greek, Persian, Macedonian,
    /// Indian, Danubian, Scythian).
    fn nation(&self, n: u8) -> String {
        const ORDER: [u16; 6] = [0, 2, 1, 4, 3, 5];
        ORDER.get(n as usize).map(|&k| self.string(31 + k)).unwrap_or_default()
    }

    /// The width of `text` in chiselled capitals.
    fn chiselled_width(&self, text: &str) -> i32 {
        text.chars()
            .map(|c| match c.to_ascii_uppercase() {
                'A'..='Z' => self.chiselled.get(c.to_ascii_uppercase() as usize - 'A' as usize).map_or(10, |g| g.width as i32),
                _ => 10,
            })
            .sum()
    }

    /// Chiselled capitals cut into what is beneath: their white pixels
    /// lighten it and their blue ones (index 252) darken it, through the
    /// palette's remap tables (🟡 which tables the original uses is not
    /// traced).
    fn draw_chiselled(&self, canvas: &mut Canvas, text: &str, x: i32, y: i32) {
        let mut x = x;
        for c in text.chars() {
            match c.to_ascii_uppercase() {
                u @ 'A'..='Z' => {
                    if let Some(g) = self.chiselled.get(u as usize - 'A' as usize) {
                        for gy in 0..g.height as i32 {
                            for gx in 0..g.width as i32 {
                                let p = g.pixels[gy as usize * g.width + gx as usize];
                                let (cx, cy) = (x + gx, y + gy);
                                if p == 0 || cx < 0 || cy < 0 || cx as usize >= canvas.width || cy as usize >= canvas.height {
                                    continue;
                                }
                                let at = cy as usize * canvas.width + cx as usize;
                                let under = canvas.pixels[at];
                                canvas.pixels[at] = if p == 252 { self.remap[0][under as usize] } else { self.remap[2][under as usize] };
                            }
                        }
                        x += g.width as i32;
                    }
                }
                _ => x += 10,
            }
        }
    }

    /// The running commentary (`INFODLG`, 164 × 47 units, a rich text box in
    /// the original) in the bottom right corner above the status bar, the
    /// size it has there (254 × 110 pixels with its caption): a marble
    /// caption with the title and the latest lines, word-wrapped, on white.
    /// (🟡 The original's sentences are in its data with mark-up for bold
    /// names; the viewer words its own.)
    fn draw_commentary(&self, canvas: &mut Canvas, lines: &[String]) {
        let (w, h) = (254, 110);
        let (x0, y0) = (canvas.width as i32 - w - 2, canvas.height as i32 - STATUS_HEIGHT - h - 2);
        let caption = 18;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                if x < 0 || y < 0 || x as usize >= canvas.width || y as usize >= canvas.height {
                    continue;
                }
                let edge = x == x0 || y == y0 || x == x0 + w - 1 || y == y0 + h - 1 || y == y0 + caption;
                let at = y as usize * canvas.width + x as usize;
                canvas.pixels[at] = if edge {
                    248
                } else if y < y0 + caption {
                    self.marble.as_ref().map_or(247, |m| m.pixels[((y - y0) as usize % m.height) * m.width + (x - x0) as usize % m.width])
                } else {
                    255
                };
            }
        }
        let title = self.strings.as_ref().and_then(|r| r.dialog("INFODLG")).map(|d| d.title).unwrap_or_default();
        let title = title.as_str();
        self.font.draw(canvas, title, x0 + (w - self.font.width(title)) / 2, y0 + 3);
        // the latest lines that fit, wrapped to the box
        let mut wrapped = Vec::new();
        for line in lines {
            let mut current = String::new();
            for word in line.split_whitespace() {
                let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
                if self.small.width(&candidate) > w - 8 && !current.is_empty() {
                    wrapped.push(std::mem::replace(&mut current, word.to_string()));
                } else {
                    current = candidate;
                }
            }
            wrapped.push(current);
        }
        let rows = ((h - caption - 4) / 12) as usize;
        let start = wrapped.len().saturating_sub(rows);
        for (k, line) in wrapped[start..].iter().enumerate() {
            self.small.draw(canvas, line, x0 + 4, y0 + caption + 2 + 12 * k as i32);
        }
    }

    /// Which of the game overview's buttons `(x, y)` is on: 1 OK, 102 the
    /// home army's details (left), 101 the visitor's.
    pub fn game_overview_at(&self, width: usize, height: usize, x: i32, y: i32) -> Option<u16> {
        let (ox, oy, _, _) = Self::centred(&self.overview_art, (539, 254), width, height);
        [(1, (128, 133, 50, 14)), (102, (12, 133, 89, 14)), (101, (205, 133, 89, 14))].into_iter().find_map(|(id, fallback)| {
            let (bx, by, bw, bh) = Self::button_rect(&self.overview_dialog, id, fallback, (ox, oy));
            (x >= bx && x < bx + bw && y >= by && y < by + bh).then_some(id)
        })
    }

    /// The end picture for `nation` (a nation byte), won or lost: the
    /// Macedonian, Persian (also the Scythians'), Greek, Danubian and Indian
    /// pictures of `EOGDLG.EIF`.
    fn ending_art(&self, (nation, won): Ending) -> Option<&Sprite> {
        let people = ["Greek", "Persian", "Macedonian", "Indian", "Danubian", "Persian"].get(nation as usize)?;
        let title = format!("{people} {}", if won { "Victory" } else { "Defeat" });
        self.endings.iter().find(|(n, _)| *n == title).map(|(_, s)| s)
    }

    fn ending_origin(&self, ending: Ending, width: usize, height: usize) -> (i32, i32) {
        let art = self.ending_art(ending).cloned();
        let (x, y, _, _) = Self::centred(&art, (474, 470), width, height);
        (x, y)
    }

    /// Which of the end of game buttons `(x, y)` is on: 101 Overview, 102
    /// View Field, 1 Close.
    pub fn ending_at(&self, ending: Ending, width: usize, height: usize, x: i32, y: i32) -> Option<u16> {
        let origin = self.ending_origin(ending, width, height);
        [(101, (59, 266, 50, 14)), (102, (106, 266, 50, 14)), (1, (155, 266, 50, 14))].into_iter().find_map(|(id, fallback)| {
            let (bx, by, bw, bh) = Self::button_rect(&self.end_dialog, id, fallback, origin);
            (x >= bx && x < bx + bw && y >= by && y < by + bh).then_some(id)
        })
    }

    /// The end of game dialog (`ENDOFGAMEDLG`, `EndOfGameDlg`): only its
    /// bitmap, "You are victorious" or "You have been defeated" (the defeat
    /// pictures, and the dialog, 15 pixels narrower), with Overview, View
    /// Field and Close. (🟡 The original's constructor loads the Macedonian
    /// picture and keeps the other nations' names unused; the viewer shows
    /// the player's own nation.)
    fn draw_ending(&self, canvas: &mut Canvas, ending: Ending) {
        let (ox, oy) = self.ending_origin(ending, canvas.width, canvas.height);
        if let Some(a) = self.ending_art(ending) {
            canvas.blit(a, ox, oy);
        }
        for c in self.end_dialog.iter().flat_map(|d| &d.controls) {
            let (bx, by, bw, bh) = du(c.rect);
            self.draw_button(canvas, (ox + bx, oy + by, bw, bh), &c.text);
        }
    }

    /// Where `p`'s dialog goes in a `width` × `height` window (caption
    /// included), and its template.
    fn plain_place(&self, p: &Plain, width: usize, height: usize) -> Option<((i32, i32, i32, i32), gboh_formats::pe::Dialog)> {
        let d = self.strings.as_ref()?.dialog(p.name)?;
        let (_, _, w, h) = du((0, 0, d.size.0, d.size.1));
        let (x, y) = ((width as i32 - w) / 2, (height as i32 - h - CAPTION) / 2);
        Some(((x, y, w, h + CAPTION), d))
    }

    /// The top left of `p`'s dialog's client area (under its caption).
    pub fn plain_origin(&self, p: &Plain, width: usize, height: usize) -> Option<(i32, i32)> {
        let ((x, y, _, _), _) = self.plain_place(p, width, height)?;
        Some((x, y + CAPTION))
    }

    /// What a click at `(x, y)` meets on `p`'s dialog: a button's or check
    /// box's id, `Some(0)` elsewhere on the dialog, `None` off it.
    pub fn plain_at(&self, p: &Plain, width: usize, height: usize, x: i32, y: i32) -> Option<u16> {
        let ((dx, dy, dw, dh), d) = self.plain_place(p, width, height)?;
        if x < dx || y < dy || x >= dx + dw || y >= dy + dh {
            return None;
        }
        use gboh_formats::pe::Name::Id;
        let hit = d
            .controls
            .iter()
            .filter(|c| c.rect.0 < d.size.0)
            .filter(|c| (c.class == Id(0x80) && !c.text.is_empty()) || matches!(c.class, Id(0x81) | Id(0x83)))
            .find(|c| {
                let (bx, by, bw, bh) = du(c.rect);
                let (bx, by) = (dx + bx, dy + CAPTION + by);
                x >= bx && x < bx + bw && y >= by && y < by + bh
            });
        Some(hit.map_or(0, |c| c.id))
    }

    /// The list box line of `p`'s dialog at `(x, y)`, if any.
    pub fn plain_line_at(&self, p: &Plain, width: usize, height: usize, x: i32, y: i32) -> Option<usize> {
        let ((dx, dy, _, _), d) = self.plain_place(p, width, height)?;
        let c = d.controls.iter().find(|c| c.class == gboh_formats::pe::Name::Id(0x83))?;
        let (bx, by, bw, bh) = du(c.rect);
        let (bx, by) = (dx + bx, dy + CAPTION + by);
        if x < bx || x >= bx + bw || y < by + 2 || y >= by + bh {
            return None;
        }
        let k = ((y - by - 2) / LIST_LINE) as usize;
        (k < p.list.len()).then_some(k)
    }

    /// A dialog with no art of its own (the `EruditeDialog`s the original
    /// pops up during play): the game's marble with a caption bar and dark
    /// frame, its static texts in the small font, its buttons, and its check
    /// boxes as a framed square (crossed when ticked) before the text.
    fn draw_plain(&self, canvas: &mut Canvas, p: &Plain) {
        let Some(((x0, y0, w, h), d)) = self.plain_place(p, canvas.width, canvas.height) else { return };
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                if x < 0 || y < 0 || x as usize >= canvas.width || y as usize >= canvas.height {
                    continue;
                }
                let edge = x == x0 || y == y0 || x == x0 + w - 1 || y == y0 + h - 1 || y == y0 + CAPTION;
                canvas.pixels[y as usize * canvas.width + x as usize] = if edge {
                    248
                } else {
                    self.marble.as_ref().map_or(247, |m| m.pixels[((y - y0) as usize % m.height) * m.width + (x - x0) as usize % m.width])
                };
            }
        }
        let title = p.texts.iter().find(|(id, _)| *id == 0).map_or(d.title.as_str(), |(_, t)| t.as_str());
        self.font.draw(canvas, title, x0 + (w - self.font.width(title)) / 2, y0 + 3);
        let top = y0 + CAPTION;
        for &(sx, sy, value) in &p.sliders {
            self.draw_slider(canvas, x0 + du((sx, sy, 0, 0)).0, top + du((sx, sy, 0, 0)).1, value);
        }
        // (controls past the dialog's right edge, as the preferences' debug
        // ones, are not shown)
        for c in d.controls.iter().filter(|c| c.rect.0 < d.size.0) {
            let (bx, by, bw, bh) = du(c.rect);
            let (bx, by) = (x0 + bx, top + by);
            let text = p.texts.iter().find(|(id, _)| *id == c.id && c.id != 0xffff).map_or(c.text.as_str(), |(_, t)| t.as_str());
            match c.class {
                gboh_formats::pe::Name::Id(0x80) if text.is_empty() => {}
                // check boxes (BS_CHECKBOX, BS_AUTOCHECKBOX)
                gboh_formats::pe::Name::Id(0x80) if matches!(c.style & 0xf, 2 | 3) => {
                    let (sx, sy) = (bx, by + (bh - 10) / 2);
                    for i in 0..10 {
                        for (px, py) in [(sx + i, sy), (sx + i, sy + 9), (sx, sy + i), (sx + 9, sy + i)] {
                            if px >= 0 && py >= 0 && (px as usize) < canvas.width && (py as usize) < canvas.height {
                                canvas.pixels[py as usize * canvas.width + px as usize] = 248;
                            }
                        }
                    }
                    if p.checked.contains(&c.id) {
                        self.small.draw(canvas, "X", sx + 2, sy - 1);
                    }
                    let caption: String = text.chars().filter(|&ch| ch != '&').collect();
                    self.small.draw(canvas, &caption, sx + 14, by + (bh - 11) / 2);
                }
                gboh_formats::pe::Name::Id(0x80) => self.draw_button(canvas, (bx, by, bw, bh), text),
                // edit boxes: white, framed, a caret where typing goes
                gboh_formats::pe::Name::Id(0x81) => {
                    fill_framed(canvas, (bx, by, bw, bh));
                    let shown = if p.focus == Some(c.id) { format!("{text}|") } else { text.to_string() };
                    self.small.draw(canvas, &shown, bx + 3, by + (bh - 11) / 2);
                }
                // the list box: white, framed, its lines with the chosen one
                // framed
                gboh_formats::pe::Name::Id(0x83) => {
                    fill_framed(canvas, (bx, by, bw, bh));
                    for (k, line) in p.list.iter().enumerate() {
                        let ly = by + 2 + LIST_LINE * k as i32;
                        if ly + LIST_LINE > by + bh {
                            break;
                        }
                        if p.selected == Some(k) {
                            frame_rect(canvas, (bx + 1, ly, bw - 2, LIST_LINE));
                        }
                        // cut to the box
                        let mut line = line.clone();
                        while self.small.width(&line) > bw - 8 && line.pop().is_some() {}
                        self.small.draw(canvas, &line, bx + 4, ly + 1);
                    }
                }
                _ => {
                    let label: String = text.chars().filter(|&ch| ch != '&').collect();
                    self.small.draw(canvas, &label, bx, by);
                }
            }
        }
    }

    /// Where the reader lies in a `width` × `height` window, and its text
    /// box.
    fn reader_place(width: usize, height: usize) -> (Rect, Rect) {
        let (w, h) = (READER.0.min(width as i32 - 8), READER.1.min(height as i32 - TOP - STATUS_HEIGHT - 8));
        let (x, y) = ((width as i32 - w) / 2, TOP + (height as i32 - TOP - STATUS_HEIGHT - h) / 2);
        ((x, y, w, h), (x + 8, y + CAPTION + 6, w - 16, h - CAPTION - 6 - 34))
    }

    /// The reader's lines as wrapped to its box: text and the paragraph
    /// each belongs to (blank lines between paragraphs).
    fn reader_lines(&self, r: &Reader, width: usize, height: usize) -> Vec<(String, Option<usize>)> {
        let (_, (_, _, tw, _)) = Self::reader_place(width, height);
        let mut lines = Vec::new();
        for (k, p) in r.paragraphs.iter().enumerate() {
            for raw in p.split('\n') {
                let raw = raw.replace('\t', "    ");
                let mut current = String::new();
                for word in raw.split(' ') {
                    let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
                    if self.small.width(&candidate) > tw - 8 && !current.is_empty() {
                        lines.push((std::mem::replace(&mut current, word.to_string()), Some(k)));
                    } else {
                        current = candidate;
                    }
                }
                lines.push((current, Some(k)));
            }
            lines.push((String::new(), None));
        }
        lines
    }

    /// How many lines the reader shows at once.
    pub fn reader_rows(width: usize, height: usize) -> usize {
        let (_, (_, _, _, th)) = Self::reader_place(width, height);
        ((th - 4) / LIST_LINE).max(1) as usize
    }

    /// How many lines the reader's text runs to.
    pub fn reader_len(&self, r: &Reader, width: usize, height: usize) -> usize {
        self.reader_lines(r, width, height).len()
    }

    fn reader_button(r: &Reader, k: usize, width: usize, height: usize) -> (i32, i32, i32, i32) {
        let ((x, y, w, h), _) = Self::reader_place(width, height);
        let n = r.buttons.len().max(1) as i32;
        let (bw, bh) = (88, 24);
        let gap = 16;
        let start = x + (w - n * bw - (n - 1) * gap) / 2;
        (start + (n - 1 - k as i32) * (bw + gap), y + h - bh - 6, bw, bh)
    }

    /// What a click at `(x, y)` meets on the reader.
    pub fn reader_at(&self, r: &Reader, width: usize, height: usize, x: i32, y: i32) -> Option<ReaderClick> {
        for k in 0..r.buttons.len() {
            let (bx, by, bw, bh) = Self::reader_button(r, k, width, height);
            if x >= bx && x < bx + bw && y >= by && y < by + bh {
                return Some(ReaderClick::Button(k));
            }
        }
        let (_, (tx, ty, tw, th)) = Self::reader_place(width, height);
        if x < tx || x >= tx + tw || y < ty + 2 || y >= ty + th {
            return None;
        }
        let row = ((y - ty - 2) / LIST_LINE) as usize + r.scroll;
        let (_, para) = self.reader_lines(r, width, height).get(row)?.clone();
        r.links.get(para?).copied().flatten().map(ReaderClick::Link)
    }

    fn draw_reader(&self, canvas: &mut Canvas, r: &Reader) {
        let ((x0, y0, w, h), (tx, ty, tw, th)) = Self::reader_place(canvas.width, canvas.height);
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                if x < 0 || y < 0 || x as usize >= canvas.width || y as usize >= canvas.height {
                    continue;
                }
                let edge = x == x0 || y == y0 || x == x0 + w - 1 || y == y0 + h - 1 || y == y0 + CAPTION;
                canvas.pixels[y as usize * canvas.width + x as usize] = if edge {
                    248
                } else {
                    self.marble.as_ref().map_or(247, |m| m.pixels[((y - y0) as usize % m.height) * m.width + (x - x0) as usize % m.width])
                };
            }
        }
        let mut title = r.title.clone();
        while self.font.width(&title) > w - 8 && title.pop().is_some() {}
        self.font.draw(canvas, &title, x0 + (w - self.font.width(&title)) / 2, y0 + 3);
        fill_framed(canvas, (tx, ty, tw, th));
        let rows = Self::reader_rows(canvas.width, canvas.height);
        for (k, (line, para)) in self.reader_lines(r, canvas.width, canvas.height).iter().skip(r.scroll).take(rows).enumerate() {
            let ly = ty + 2 + LIST_LINE * k as i32;
            let linked = para.and_then(|p| r.links.get(p).copied().flatten()).is_some();
            if linked {
                // links underlined, as WinHelp shows its jumps
                let lw = self.small.width(line);
                for px in tx + 4..tx + 4 + lw {
                    if (px as usize) < canvas.width && ((ly + 12) as usize) < canvas.height {
                        canvas.pixels[(ly + 12) as usize * canvas.width + px as usize] = 2;
                    }
                }
                self.small.draw_in(canvas, line, tx + 4, ly + 1, 2);
            } else {
                self.small.draw(canvas, line, tx + 4, ly + 1);
            }
        }
        for (k, b) in r.buttons.iter().enumerate() {
            self.draw_button(canvas, Self::reader_button(r, k, canvas.width, canvas.height), b);
        }
    }

    /// The width of a slider's track.
    fn slider_width(&self) -> i32 {
        self.slider.iter().skip(1).map(|c| c.width as i32).sum()
    }

    /// A slider: its track, and the thumb along it at `value`.
    fn draw_slider(&self, canvas: &mut Canvas, x: i32, y: i32, value: u16) {
        let [thumb, left, middle, right] = self.slider.as_slice() else { return };
        canvas.blit(left, x, y);
        canvas.blit(middle, x + left.width as i32, y);
        canvas.blit(right, x + (left.width + middle.width) as i32, y);
        let room = self.slider_width() - thumb.width as i32;
        canvas.blit(thumb, x + room * value as i32 / SLIDER_MAX as i32, y);
    }

    /// The slider of `p`'s dialog at `(x, y)` and the value there, if one.
    pub fn plain_slider_at(&self, p: &Plain, width: usize, height: usize, x: i32, y: i32) -> Option<(usize, u16)> {
        let ((dx, dy, _, _), _) = self.plain_place(p, width, height)?;
        let thumb = self.slider.first()?.width as i32;
        let (w, h) = (self.slider_width(), self.slider.first()?.height as i32);
        p.sliders.iter().enumerate().find_map(|(k, &(sx, sy, _))| {
            let (sx, sy, _, _) = du((sx, sy, 0, 0));
            let (sx, sy) = (dx + sx, dy + CAPTION + sy);
            (x >= sx && x < sx + w && y >= sy && y < sy + h).then(|| {
                let v = ((x - sx - thumb / 2) * SLIDER_MAX as i32 / (w - thumb).max(1)).clamp(0, SLIDER_MAX as i32);
                (k, v as u16)
            })
        })
    }

    /// The value slider `k` of `p`'s dialog takes with the mouse at `x`
    /// (dragging: wherever the mouse is up or down).
    pub fn plain_slider_value(&self, p: &Plain, width: usize, height: usize, k: usize, x: i32) -> Option<u16> {
        let ((dx, _, _, _), _) = self.plain_place(p, width, height)?;
        let thumb = self.slider.first()?.width as i32;
        let &(sx, sy, _) = p.sliders.get(k)?;
        let sx = dx + du((sx, sy, 0, 0)).0;
        Some(((x - sx - thumb / 2) * SLIDER_MAX as i32 / (self.slider_width() - thumb).max(1)).clamp(0, SLIDER_MAX as i32) as u16)
    }

    /// Only `p`'s dialog, on a clear `width` × `height` overlay (over the
    /// start screen).
    pub fn dialog_canvas(&self, width: usize, height: usize, p: &Plain) -> Canvas {
        let mut canvas = Canvas::new(width, height, 0);
        self.draw_plain(&mut canvas, p);
        canvas
    }

    /// Whether `(x, y)` is on the army details' Close button.
    pub fn army_close_at(&self, width: usize, height: usize, x: i32, y: i32) -> bool {
        let (ox, oy, _, _) = Self::centred(&self.army_art, (346, 470), width, height);
        let (bx, by, bw, bh) = Self::button_rect(&self.army_dialog, 1, (43, 269, 50, 14), (ox, oy));
        x >= bx && x < bx + bw && y >= by && y < by + bh
    }

    /// The army details (`ARMYDETAILSDLG`, `ArmyDetailsDlg`; painted by
    /// `FUN_00583de7` on the `Army Details` bitmap): "<nation> Army" centred
    /// on x 170 at y 52, then at x 28 from y 76, 24 apart, the rout level,
    /// the percentage of it eliminated, the TQ routing and the rout points
    /// eliminated (strings 123–126); below the template's column heads, a
    /// rich text list at (16, 276), 309 × 132, filled by `FUN_005845f7`:
    /// each unit's name (bold, cut to 23 characters), TQ and hits, and the
    /// army's leaders with `-` for both, those out of the battle with `X` and
    /// their rout points, at tab stops 189, 216, 243 and 270. (🟡 The bold
    /// system font and rich text are drawn in the game's small font.)
    fn draw_army(&self, canvas: &mut Canvas, a: &ArmyDetails) {
        let (ox, oy, _, _) = Self::centred(&self.army_art, (346, 470), canvas.width, canvas.height);
        if let Some(art) = &self.army_art {
            canvas.blit(art, ox, oy);
        }
        let title = format!("{} Army", self.nation(a.nation));
        self.small.draw(canvas, &title, ox + 170 - self.small.width(&title) / 2, oy + 52);
        let percent = if a.level == 0 { 0 } else { a.eliminated.min(a.level) as u32 * 100 / a.level as u32 };
        let format = |id: u16, n: u32| self.string(id).replacen("%d", &n.to_string(), 1).replace("%%", "%");
        for (k, line) in
            [format(123, a.level as u32), format(124, percent), format(125, a.routing_tq as u32), format(126, a.eliminated as u32)].iter().enumerate()
        {
            self.small.draw(canvas, line, ox + 28, oy + 76 + 24 * k as i32);
        }
        // the column heads and buttons of the template
        for c in self.army_dialog.iter().flat_map(|d| &d.controls) {
            let (bx, by, bw, bh) = du(c.rect);
            if c.class == gboh_formats::pe::Name::Id(0x80) {
                self.draw_button(canvas, (ox + bx, oy + by, bw, bh), &c.text);
            } else {
                self.small.draw(canvas, &c.text, ox + bx, oy + by);
            }
        }
        // the list: white, framed
        let (lx, ly, lw, lh) = (ox + 16, oy + 276, 309, 132);
        for y in ly..ly + lh {
            for x in lx..lx + lw {
                if x < 0 || y < 0 || x as usize >= canvas.width || y as usize >= canvas.height {
                    continue;
                }
                let edge = x == lx || y == ly || x == lx + lw - 1 || y == ly + lh - 1;
                canvas.pixels[y as usize * canvas.width + x as usize] = if edge { 248 } else { 255 };
            }
        }
        let tabs = [189, 216, 243, 270];
        for (k, row) in a.rows.iter().skip(a.scroll).take(ARMY_LINES).enumerate() {
            let y = ly + 4 + 12 * k as i32;
            let name: String = row.name.chars().take(23).collect();
            self.small.draw(canvas, &name, lx + 3, y);
            let tq = row.tq.map_or("-".to_string(), |t| t.to_string());
            let hits = row.hits.map_or("-".to_string(), |h| h.to_string());
            self.small.draw(canvas, &tq, lx + tabs[0], y);
            self.small.draw(canvas, &hits, lx + tabs[1], y);
            if let Some(lost) = row.lost {
                self.small.draw(canvas, "X", lx + tabs[2], y);
                self.small.draw(canvas, &lost.to_string(), lx + tabs[3], y);
            }
        }
    }

    /// The game overview (`GAMEOVERVIEWDLG`, drawn by `FUN_0058f6de` on the
    /// `End of Game` bitmap): each army's nation in chiselled capitals
    /// centred a quarter and three quarters across at y 30, and at x 40 and
    /// 330 from y 75, a line apart, its rout level, the rout points lost and
    /// those remaining (strings 75–77). (🟡 The original writes in the bold
    /// system font; its Army Details buttons are not wired here.)
    fn draw_game_overview(&self, canvas: &mut Canvas, g: &GameOverview) {
        let (ox, oy, w, _) = Self::centred(&self.overview_art, (539, 254), canvas.width, canvas.height);
        if let Some(a) = &self.overview_art {
            canvas.blit(a, ox, oy);
        }
        for (side, centre) in [w / 4, w * 3 / 4].into_iter().enumerate() {
            let name = self.nation(g.nations[side]).to_uppercase();
            self.draw_chiselled(canvas, &name, ox + centre - self.chiselled_width(&name) / 2, oy + 30);
            let x = ox + if side == 0 { 40 } else { 330 };
            let remaining = g.levels[side] as i32 - g.lost[side] as i32;
            let lines =
                [format!("{}{}", self.string(75), g.levels[side]), format!("{}{}", self.string(76), g.lost[side]), format!("{}{}", self.string(77), remaining)];
            for (k, line) in lines.iter().enumerate() {
                self.small.draw(canvas, line, x, oy + 75 + 16 * k as i32 - 2);
            }
        }
        for c in self.overview_dialog.iter().flat_map(|d| &d.controls) {
            let (bx, by, bw, bh) = du(c.rect);
            self.draw_button(canvas, (ox + bx, oy + by, bw, bh), &c.text);
        }
    }

    /// Where a dialog of `art`'s size lies, centred on a `width` × `height`
    /// window (the overall results a little to the left of the details).
    fn centred(art: &Option<Sprite>, fallback: (i32, i32), width: usize, height: usize) -> (i32, i32, i32, i32) {
        let (w, h) = art.as_ref().map_or(fallback, |a| (a.width as i32, a.height as i32));
        ((width as i32 - w) / 2, (height as i32 - h) / 2, w, h)
    }

    /// The overall results' list: its rectangle and one line's height.
    fn overall_list(&self, width: usize, height: usize) -> ((i32, i32, i32, i32), i32) {
        let (ox, oy, _, _) = Self::centred(&self.overall_art, (230, 202), width, height);
        let list = self.overall_dialog.as_ref().and_then(|d| d.controls.iter().find(|c| c.id == 101)).map_or((23, 22, 91, 48), |c| c.rect);
        let (x, y, w, h) = du(list);
        ((ox + x, oy + y, w, h), 13)
    }

    fn button_rect(dialog: &Option<gboh_formats::pe::Dialog>, id: u16, fallback: (i16, i16, i16, i16), origin: (i32, i32)) -> (i32, i32, i32, i32) {
        let r = dialog.as_ref().and_then(|d| d.controls.iter().find(|c| c.id == id)).map_or(fallback, |c| c.rect);
        let (x, y, w, h) = du(r);
        (origin.0 + x, origin.1 + y, w, h)
    }

    /// What a click at `(x, y)` on the combat results does, if anything.
    pub fn results_at(&self, r: &Results, width: usize, height: usize, x: i32, y: i32) -> Option<ResultsClick> {
        let inside = |(bx, by, bw, bh): (i32, i32, i32, i32)| x >= bx && x < bx + bw && y >= by && y < by + bh;
        if r.open.is_some() {
            let (ox, oy, _, _) = Self::centred(&self.results_art, (551, 220), width, height);
            return inside(Self::button_rect(&self.results_dialog, 1, (131, 118, 50, 14), (ox, oy))).then_some(ResultsClick::CloseDetails);
        }
        let (ox, oy, _, _) = Self::centred(&self.overall_art, (230, 202), width, height);
        if inside(Self::button_rect(&self.overall_dialog, 1, (41, 108, 50, 14), (ox, oy))) {
            return Some(ResultsClick::Close);
        }
        let ((lx, ly, lw, lh), line) = self.overall_list(width, height);
        (x >= lx && x < lx + lw && y >= ly && y < ly + lh).then(|| ((y - ly) / line) as usize).filter(|&k| k < r.battles).map(ResultsClick::Battle)
    }

    /// Whether the left button is held down inside `rect`.
    fn held(&self, (x, y, w, h): (i32, i32, i32, i32)) -> bool {
        self.pressed.get().is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h)
    }

    /// A push button, pressed (its second cel, the caption a pixel down and
    /// right) while the left button is held on it.
    fn draw_button(&self, canvas: &mut Canvas, (bx, by, bw, bh): (i32, i32, i32, i32), caption: &str) {
        let art = if bw > 120 { &self.big_button } else { &self.button };
        let down = self.held((bx, by, bw, bh)) as i32;
        if let Some(b) = art.get(down as usize).or(art.first()) {
            canvas.blit(b, bx + (bw - b.width as i32) / 2, by + (bh - b.height as i32) / 2);
        }
        let caption: String = caption.chars().filter(|&ch| ch != '&').collect();
        self.font.draw(canvas, &caption, bx + (bw - self.font.width(&caption)) / 2 + down, by + (bh - 11) / 2 + down);
    }

    /// The combat results: the overall list of battles (`OVERALLCOMBATRESULTS`,
    /// "Battle #n" lines in its list box), and over it the chosen battle's
    /// table (`SHOWCOMBATRESULTS`, drawn by `FUN_0058c097` on the `Combat
    /// Results` bitmap): from y 100, a line apart (16 pixels as measured),
    /// attackers' names at x 10 and their hits this combat, in all and to
    /// rout at x 170, 210 and 243; defenders' at x 290, 445, 485 and 520;
    /// names marked `* ` when routed and `** ` when eliminated (🟡 which
    /// mark is which is inferred from captures), wrapped past 140 pixels.
    fn draw_results(&self, canvas: &mut Canvas, r: &Results) {
        let (width, height) = (canvas.width, canvas.height);
        let (ox, oy, _, _) = Self::centred(&self.overall_art, (230, 202), width, height);
        if let Some(a) = &self.overall_art {
            canvas.blit(a, ox, oy);
        }
        let ((lx, ly, lw, lh), line) = self.overall_list(width, height);
        // the list box: white with a dark frame, the chosen line framed
        for yy in ly..ly + lh {
            for xx in lx..lx + lw {
                if xx >= 0 && yy >= 0 && (xx as usize) < width && (yy as usize) < height {
                    let edge = xx == lx || yy == ly || xx == lx + lw - 1 || yy == ly + lh - 1;
                    canvas.pixels[yy as usize * width + xx as usize] = if edge { 248 } else { 255 };
                }
            }
        }
        for k in 0..r.battles {
            let y = ly + 2 + k as i32 * line;
            if y + line > ly + lh {
                break;
            }
            if r.selected == Some(k) {
                for xx in lx + 1..lx + lw - 1 {
                    for yy in y..y + line {
                        canvas.pixels[yy as usize * width + xx as usize] = 252;
                    }
                }
            }
            self.small.draw(canvas, &format!("Battle #{}", k + 1), lx + 4, y + 1);
        }
        self.draw_button(canvas, Self::button_rect(&self.overall_dialog, 1, (41, 108, 50, 14), (ox, oy)), "&Close");
        let Some((attackers, defenders)) = &r.open else { return };
        let (ox, oy, _, _) = Self::centred(&self.results_art, (551, 220), width, height);
        if let Some(a) = &self.results_art {
            canvas.blit(a, ox, oy);
        }
        let mut put = |text: &str, x: i32, y: i32| self.small.draw(canvas, text, ox + x, oy + y);
        for (rows, name_x, cols) in [(attackers, 10, [170, 210, 243]), (defenders, 290, [445, 485, 520])] {
            let mut y = 100;
            for row in rows {
                let mut name = String::new();
                if row.routed {
                    name.push_str("* ");
                }
                if row.eliminated {
                    name.push_str("** ");
                }
                name.push_str(&row.name);
                put(&name, name_x, y);
                for (v, x) in [row.current, row.total, row.to_rout.max(0)].into_iter().zip(cols) {
                    put(&v.to_string(), x, y);
                }
                y += 16;
            }
        }
        self.draw_button(canvas, Self::button_rect(&self.results_dialog, 1, (131, 118, 50, 14), (ox, oy)), "&Close");
    }

    fn string(&self, id: u16) -> String {
        self.strings.as_ref().and_then(|r| r.string(id)).unwrap_or_default()
    }

    /// The unit details window's origin on a `width` × `height` window
    /// (centred) and its size.
    fn details_place(&self, width: usize, height: usize) -> (i32, i32, i32, i32) {
        let (w, h) = self.details_art.as_ref().map_or((434, 314), |a| (a.width as i32, a.height as i32));
        ((width as i32 - w) / 2, (height as i32 - h) / 2, w, h)
    }

    /// Whether `(x, y)` is on the details window's OK button.
    pub fn details_ok_at(&self, width: usize, height: usize, x: i32, y: i32) -> bool {
        let (ox, oy, _, _) = self.details_place(width, height);
        let ok = self.details_dialog.as_ref().and_then(|d| d.controls.iter().find(|c| c.id == 1)).map_or((67, 174, 50, 14), |c| c.rect);
        let (bx, by, bw, bh) = (ox + ok.0 as i32 * 7 / 4, oy + ok.1 as i32 * 13 / 8, ok.2 as i32 * 7 / 4, ok.3 as i32 * 13 / 8);
        x >= bx && x < bx + bw && y >= by && y < by + bh
    }

    /// Draw the unit details window (`ViewUnitDlg`, its paint
    /// `FUN_005a0ac9`): labels at x 60 and values at x 160 down the left
    /// (rows 17 pixels apart from y 79: used this turn, condition, troop
    /// quality, cohesion hits, then movement with allowance and remaining
    /// indented to 83, then size), missile information at x 251 with values
    /// at 322 down the right. (🟡 The original writes in bold MS Sans
    /// Serif; here the game's bitmap font. Where it shows the unit's name is
    /// not traced: here at the top.)
    fn draw_details(&self, canvas: &mut Canvas, d: &Details) {
        let (ox, oy, w, _) = self.details_place(canvas.width, canvas.height);
        if let Some(a) = &self.details_art {
            canvas.blit(a, ox, oy);
        }
        let label = |id: u16| format!("{}{}", self.string(id), self.string(68));
        let mut put = |text: &str, x: i32, y: i32| self.small.draw(canvas, text, ox + x, oy + y - 10);
        put(&d.name, (w - self.small.width(&d.name)) / 2, 54);
        let yes_no = |b: bool| self.string(if b { 49 } else { 50 });
        put(&label(48), 60, 79);
        put(&yes_no(d.used), 160, 79);
        put(&label(51), 60, 96);
        put(&self.string(if d.routing { 53 } else { 52 }), 160, 96);
        put(&label(54), 60, 113);
        put(&d.tq.to_string(), 160, 113);
        put(&label(55), 60, 130);
        put(&d.hits.to_string(), 160, 130);
        put(&self.string(56), 60, 147);
        put(&label(57), 83, 164);
        put(&d.mp_allowance.to_string(), 160, 164);
        put(&label(58), 83, 181);
        put(&d.mp_left.to_string(), 160, 181);
        put(&label(59), 60, 198);
        put(&d.size.to_string(), 160, 198);
        put(&self.string(60), 251, 79);
        put(&label(61), 251, 96);
        put(&label(62), 251, 113);
        put(&label(57), 251, 130);
        put(&label(58), 251, 147);
        let na = self.string(63);
        let (kind, range, supply, left) = match d.missile {
            Some((kind, range, supply, left)) => (self.string(kind), range.to_string(), supply.to_string(), left.to_string()),
            None => (na.clone(), na.clone(), na.clone(), na),
        };
        for (text, y) in [(kind, 96), (range, 113), (supply, 130), (left, 147)] {
            put(&text, 322, y);
        }
        for c in self.details_dialog.iter().flat_map(|d| &d.controls) {
            let (bx, by) = (ox + c.rect.0 as i32 * 7 / 4, oy + c.rect.1 as i32 * 13 / 8);
            let (bw, bh) = (c.rect.2 as i32 * 7 / 4, c.rect.3 as i32 * 13 / 8);
            if let Some(b) = self.button.first() {
                canvas.blit(b, bx + (bw - b.width as i32) / 2, by + (bh - b.height as i32) / 2);
            }
            let caption: String = c.text.chars().filter(|&ch| ch != '&').collect();
            self.font.draw(canvas, &caption, bx + (bw - self.font.width(&caption)) / 2, by + (bh - 11) / 2);
        }
    }

    /// The overview scroll for `battle` and where its map lies on a `width`
    /// pixel wide window: scroll origin, map origin and map size.
    fn overview_place(&self, battle: &str, width: usize, reversed: bool) -> Option<(&Sprite, Point, Point, Point)> {
        let cels = &self.overviews.iter().find(|(n, _)| n == battle)?.1;
        let (scroll, map) = (cels.get(reversed as usize)?, cels.get(2)?);
        let at = (width as i32 - scroll.width as i32 - 4, TOP + 4);
        let map_at = (at.0 + MAP_IN_SCROLL.0, at.1 + MAP_IN_SCROLL.1);
        Some((scroll, at, map_at, (map.width as i32, map.height as i32)))
    }

    /// Where on the battlefield (0 to 1 across and down) the overview map
    /// for `battle` is at window pixel `(x, y)`, if it is there.
    pub fn overview_at(&self, battle: &str, width: usize, x: i32, y: i32) -> Option<(f32, f32)> {
        let (_, _, (mx, my), (mw, mh)) = self.overview_place(battle, width, false)?;
        (x >= mx && x < mx + mw && y >= my && y < my + mh).then(|| ((x - mx) as f32 / mw as f32, (y - my) as f32 / mh as f32))
    }

    /// A button's tooltip and status line (`shown`: the overview showing,
    /// the grid on, which pick the other texts of those two).
    pub fn tip(&self, tool: Tool, overview_shown: bool, grid_on: bool) -> Option<(String, String)> {
        match tool {
            Tool::Overview if !overview_shown => Some(self.other_tips[0].clone()),
            Tool::HexGrid if grid_on => Some(self.other_tips[1].clone()),
            _ => self.tips.iter().find(|(t, _, _)| *t == tool).map(|(_, s, l)| (s.clone(), l.clone())),
        }
        .filter(|(s, _)| !s.is_empty())
    }

    /// A Windows tooltip: the small font on pale yellow in a dark frame,
    /// below and right of the mouse, kept in the window.
    fn draw_tooltip(&self, canvas: &mut Canvas, text: &str, (x, y): (i32, i32)) {
        let (w, h) = (self.small.width(text) + 6, 15);
        let x = x.min(canvas.width as i32 - w - 1).max(0);
        let y = (y + 20).min(canvas.height as i32 - h - 1);
        for py in y..y + h {
            for px in x..x + w {
                if px >= 0 && py >= 0 && (px as usize) < canvas.width && (py as usize) < canvas.height {
                    canvas.pixels[py as usize * canvas.width + px as usize] = self.pale_yellow;
                }
            }
        }
        frame_rect(canvas, (x, y, w, h));
        self.small.draw(canvas, text, x + 3, y + 1);
    }

    /// The toolbar button at `(x, y)` (window pixels at one battlefield
    /// pixel each).
    pub fn tool_at(&self, x: i32, y: i32) -> Option<Tool> {
        self.toolbar.buttons.iter().find(|&&(_, _, _, cx)| (x - cx).abs() < 12 && (TOOLBAR_TOP + 4..TOOLBAR_TOP + 26).contains(&y)).map(|t| t.0)
    }

    /// The bars for a `width` × `height` window: buttons that `enabled`
    /// rejects are drawn greyed; `panes` are the status bar's three texts.
    #[allow(clippy::too_many_arguments)]
    pub fn canvas(
        &self,
        width: usize,
        height: usize,
        enabled: impl Fn(Tool) -> bool,
        panes: [&str; 3],
        overview: Option<&Overview>,
        details: Option<&Details>,
        results: Option<&Results>,
        game_overview: Option<&GameOverview>,
        commentary: &[String],
        army: Option<&ArmyDetails>,
        ending: Option<Ending>,
        plain: Option<&Plain>,
        reader: Option<&Reader>,
    ) -> Canvas {
        let mut canvas = Canvas::new(width, height, 0);
        if let Some(o) = overview
            && let Some((scroll, at, (mx, my), (mw, mh))) = self.overview_place(o.battle, width, o.reversed)
        {
            canvas.blit(scroll, at.0, at.1);
            let mut put = |x: i32, y: i32, c: u8| {
                if x >= mx && y >= my && x < mx + mw && y < my + mh && (x as usize) < width && (y as usize) < height {
                    canvas.pixels[y as usize * width + x as usize] = c;
                }
            };
            for &(fx, fy, c) in &o.dots {
                let (x, y) = (mx + (fx * mw as f32) as i32, my + (fy * mh as f32) as i32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    put(x + dx, y + dy, c);
                }
            }
            // the part in view, in white
            let [l, t, r, b] = o.view.map(|v| v.clamp(0.0, 1.0));
            let (x0, y0, x1, y1) = (mx + (l * mw as f32) as i32, my + (t * mh as f32) as i32, mx + (r * mw as f32) as i32 - 1, my + (b * mh as f32) as i32 - 1);
            for x in x0..=x1 {
                put(x, y0, 255);
                put(x, y1, 255);
            }
            for y in y0..=y1 {
                put(x0, y, 255);
                put(x1, y, 255);
            }
        }
        // the toolbar's marble strip
        if let Some(m) = &self.marble {
            for x in (0..width as i32).step_by(m.width) {
                for row in 0..TOOLBAR_HEIGHT.min(m.height as i32) {
                    for col in 0..m.width as i32 {
                        let (cx, cy) = (x + col, TOOLBAR_TOP + row);
                        if cx < width as i32 && (cy as usize) < height {
                            canvas.pixels[cy as usize * width + cx as usize] = m.pixels[row as usize * m.width + col as usize];
                        }
                    }
                }
            }
        }
        for (&(tool, _, _, cx), cels) in self.toolbar.buttons.iter().zip(&self.buttons) {
            // up, down, disabled, disabled down
            let down = self.held((cx - 12, TOOLBAR_TOP + 4, 24, 22)) as usize;
            if let Some(c) = cels.get(if enabled(tool) { 0 } else { 2 } + down) {
                canvas.blit(c, cx - 12, TOOLBAR_TOP + 4);
            }
        }
        // the status bar: caps, tiled middle, dividers before the last two
        // panes (as in the original, about three quarters and five sixths
        // of the way)
        let y = height as i32 - STATUS_HEIGHT;
        if let [left, middle, right, divider] = self.bar.as_slice() {
            let w = width as i32;
            let mut x = 0;
            while x < w {
                canvas.blit(middle, x, y);
                x += middle.width as i32;
            }
            canvas.blit(left, 0, y);
            canvas.blit(right, w - right.width as i32, y);
            let cuts = [w * 3 / 4, w * 6 / 7];
            for &c in &cuts {
                canvas.blit(divider, c - divider.width as i32 / 2, y);
            }
            let starts = [left.width as i32, cuts[0] + divider.width as i32 / 2, cuts[1] + divider.width as i32 / 2];
            for (text, x0) in panes.iter().zip(starts) {
                self.font.draw(&mut canvas, text, x0 + 4, y + 18);
            }
        }
        if !commentary.is_empty() {
            self.draw_commentary(&mut canvas, commentary);
        }
        if let Some(d) = details {
            self.draw_details(&mut canvas, d);
        }
        if let Some(r) = results {
            self.draw_results(&mut canvas, r);
        }
        if let Some(e) = ending {
            self.draw_ending(&mut canvas, e);
        }
        if let Some(g) = game_overview {
            self.draw_game_overview(&mut canvas, g);
        }
        if let Some(a) = army {
            self.draw_army(&mut canvas, a);
        }
        if let Some(r) = reader {
            self.draw_reader(&mut canvas, r);
        }
        if let Some(p) = plain {
            self.draw_plain(&mut canvas, p);
        }
        if let Some((text, at)) = self.tooltip.borrow().as_ref() {
            self.draw_tooltip(&mut canvas, text, *at);
        }
        canvas
    }

    /// Only the reader, on a clear overlay (over the start screen).
    pub fn reader_canvas(&self, width: usize, height: usize, r: &Reader) -> Canvas {
        let mut canvas = Canvas::new(width, height, 0);
        self.draw_reader(&mut canvas, r);
        canvas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Caesar's toolbar: every button's art and texts are there; runs only
    /// with `GBOH_CAESAR` set.
    #[test]
    fn caesar_toolbar() {
        let Ok(dir) = std::env::var("GBOH_CAESAR") else { return };
        let hud = Hud::new(Path::new(&dir));
        assert_eq!(hud.buttons.len(), 24);
        assert!(hud.buttons.iter().all(|b| b.len() >= 2));
        assert!(hud.tips.iter().all(|(_, short, long)| !short.is_empty() && !long.is_empty()), "{:?}", hud.tips);
        assert!(hud.tips.iter().any(|(t, _, _)| *t == Tool::GroupAttack));
        // up cels first (Caesar's files list the down one first)
        let c = hud.canvas(640, 120, |_| true, ["", "", ""], None, None, None, None, &[], None, None, None, None);
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(crate::palette_path(Path::new(&dir)).unwrap()).unwrap()).unwrap();
            crate::ui::write_ppm(&c, &pal, &Path::new(&out).join("cae_toolbar.ppm")).unwrap();
        }
    }

    /// Draws the bars with an overview map; runs only with `GBOH_DATA` set
    /// (and writes them to `GBOH_SHOTS`, if set).
    #[test]
    fn draws_the_bars() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let hud = Hud::new(Path::new(&dir));
        let dots = (0..40).map(|i| (0.2 + (i % 8) as f32 * 0.08, 0.3 + (i / 8) as f32 * 0.1, if i < 20 { 249 } else { 252 })).collect();
        let overview = Overview { battle: "Issus", dots, view: [0.1, 0.2, 0.5, 0.6], reversed: false };
        let details = Details {
            name: "Hypaspists 1".into(),
            used: false,
            routing: false,
            tq: 7,
            hits: 2,
            mp_allowance: 6,
            mp_left: 6,
            size: 1,
            missile: Some((66, 2, 6, 4)),
        };
        let c = hud.canvas(
            800,
            500,
            |t| t != Tool::Undo,
            ["Turn 1 Alexander III", "Turn 1", "Normal"],
            Some(&overview),
            Some(&details),
            None,
            None,
            &[
                "--- Turn #1 ---".to_string(),
                "Alexander III is now active.".to_string(),
                "Agrian 1 shoots at India 4. India 4 takes 2 cohesion hits and is routing!".to_string(),
            ],
            None,
            None,
            None,
            None,
        );
        // the combat results: two battles, the first open
        let row = |name: &str, current, total, to_rout, routed| ResultRow { name: name.into(), current, total, to_rout, routed, eliminated: false };
        let results = Results {
            battles: 2,
            selected: Some(0),
            open: Some((vec![row("Mercenary 2", 2, 3, 2, false), row("Agema Hypaspist", 2, 3, 6, false)], vec![row("India 10", 2, 3, 2, true)])),
        };
        let r = hud.canvas(800, 500, |_| true, ["", "", ""], None, None, Some(&results), None, &[], None, None, None, None);
        let g = hud.canvas(
            800,
            500,
            |_| true,
            ["", "", ""],
            None,
            None,
            None,
            Some(&GameOverview { nations: [2, 3], levels: [80, 70], lost: [1, 38] }),
            &[],
            None,
            None,
            None,
            None,
        );
        let rows = (0..14)
            .map(|k| ArmyRow {
                name: format!("Hypaspist {}", k + 1),
                tq: (k < 12).then_some(7),
                hits: (k < 12).then_some(k % 4),
                lost: (k % 5 == 1).then_some(6),
            })
            .collect();
        let army = ArmyDetails { nation: 2, level: 80, routing_tq: 7, eliminated: 18, rows, scroll: 0 };
        let a = hud.canvas(800, 600, |_| true, ["", "", ""], None, None, None, None, &[], Some(&army), None, None, None);
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(Path::new(&dir).join("GAME.PAL")).unwrap()).unwrap();
            let rgba = a.to_rgba(&pal);
            let mut ppm = format!("P6 {} {} 255\n", a.width, a.height).into_bytes();
            for p in rgba.chunks(4) {
                ppm.extend_from_slice(&p[..3]);
            }
            std::fs::write(format!("{out}/army.ppm"), ppm).unwrap();
        }
        let e = hud.canvas(800, 600, |_| true, ["", "", ""], None, None, None, None, &[], None, Some((2, false)), None, None);
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(Path::new(&dir).join("GAME.PAL")).unwrap()).unwrap();
            let rgba = e.to_rgba(&pal);
            let mut ppm = format!("P6 {} {} 255\n", e.width, e.height).into_bytes();
            for p in rgba.chunks(4) {
                ppm.extend_from_slice(&p[..3]);
            }
            std::fs::write(format!("{out}/ending.ppm"), ppm).unwrap();
        }
        assert!(hud.ending_art((5, true)).is_some(), "the Scythians end with the Persian pictures");
        if hud.strings.is_some() {
            let missile = Plain {
                name: "MISSILERESULTS",
                texts: vec![(101, "India 4 suffers 2 cohesion hits".into()), (102, "and is routing!".into())],
                checked: vec![201],
                ..Default::default()
            };
            let momentum = Plain { name: "MOMENTUMDLG", ..Default::default() };
            let (ww, wh) = (640, 400);
            let files = Plain {
                name: "LOADSAVEDLG",
                texts: vec![(0, "Save Game".into()), (10, "The Battle of Hydaspes; Turn #3; Coenus".into()), (12, "Save1".into())],
                list: vec!["Autosave.sav  Autosaved Game: The Battle of Hydaspes; Turn #3".into(), "Save1.sav  The Battle of Issus; Turn #5".into()],
                selected: Some(1),
                focus: Some(12),
                ..Default::default()
            };
            let reader = Reader {
                title: "Hydaspes  (326 BC)".into(),
                paragraphs: vec![
                    "STANDARD VICTORY CONDITIONS: (Can be altered with the Army Rout Level dialog box)".into(),
                    "Indians: The Indians win if they can rout the Macedonian army, or if they are unrouted by the conclusion of game turn 10 of the battle."
                        .into(),
                    "The Armies at Hydaspes".into(),
                ],
                links: vec![None, None, Some(3)],
                scroll: 0,
                buttons: vec!["&Close".into(), "Contents".into()],
            };
            let rc = hud.reader_canvas(640, 480, &reader);
            if let Ok(out) = std::env::var("GBOH_SHOTS") {
                let pal = gboh_formats::pal::Palette::parse(&std::fs::read(Path::new(&dir).join("GAME.PAL")).unwrap()).unwrap();
                crate::ui::write_ppm(&rc, &pal, &Path::new(&out).join("reader.ppm")).unwrap();
            }
            let ((_, _, _, _), (tx, ty, _, _)) = Hud::reader_place(640, 480);
            let link_row = ty + 2 + LIST_LINE * (hud.reader_lines(&reader, 640, 480).iter().position(|(_, p)| *p == Some(2)).unwrap() as i32) + 3;
            assert_eq!(hud.reader_at(&reader, 640, 480, tx + 10, link_row), Some(ReaderClick::Link(3)));
            let prefs = crate::prefs::Prefs { combat_animation: false, ..Default::default() }.dialog(gboh_formats::game::Game::Alexander);
            for (k, p) in [&missile, &momentum, &files, &prefs].into_iter().enumerate() {
                let c = hud.canvas(ww, wh, |_| true, ["", "", ""], None, None, None, None, &[], None, None, Some(p), None);
                if let Ok(out) = std::env::var("GBOH_SHOTS") {
                    let pal = gboh_formats::pal::Palette::parse(&std::fs::read(Path::new(&dir).join("GAME.PAL")).unwrap()).unwrap();
                    let rgba = c.to_rgba(&pal);
                    let mut ppm = format!("P6 {} {} 255\n", c.width, c.height).into_bytes();
                    for px in rgba.chunks(4) {
                        ppm.extend_from_slice(&px[..3]);
                    }
                    std::fs::write(format!("{out}/plain{k}.ppm"), ppm).unwrap();
                }
            }
            // the momentum question's Yes and No, and a click off it
            let ((x, y, _, _), _) = hud.plain_place(&momentum, ww, wh).unwrap();
            let at = |bx: i32, by: i32| hud.plain_at(&momentum, ww, wh, x + bx * 7 / 4 + 4, y + CAPTION + by * 13 / 8 + 4);
            assert_eq!((at(134, 6), at(134, 24), at(3, 8)), (Some(1), Some(2), Some(0)));
            assert_eq!(hud.plain_at(&momentum, ww, wh, 2, 2), None);
        }
        let (ox, oy) = hud.ending_origin((2, false), 800, 600);
        assert_eq!(hud.ending_at((2, false), 800, 600, ox + 155 * 7 / 4 + 5, oy + 266 * 13 / 8 + 5), Some(1));
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(Path::new(&dir).join("GAME.PAL")).unwrap()).unwrap();
            let rgba = g.to_rgba(&pal);
            let mut ppm = format!("P6 {} {} 255\n", g.width, g.height).into_bytes();
            for p in rgba.chunks(4) {
                ppm.extend_from_slice(&p[..3]);
            }
            std::fs::write(format!("{out}/game_overview.ppm"), ppm).unwrap();
        }
        let closed = Results { open: None, ..results };
        let ((lx, ly, _, _), _) = hud.overall_list(800, 500);
        assert_eq!(hud.results_at(&closed, 800, 500, lx + 5, ly + 3 + 13), Some(ResultsClick::Battle(1)));
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(Path::new(&dir).join("GAME.PAL")).unwrap()).unwrap();
            let rgba = r.to_rgba(&pal);
            let mut ppm = format!("P6 {} {} 255\n", r.width, r.height).into_bytes();
            for p in rgba.chunks(4) {
                ppm.extend_from_slice(&p[..3]);
            }
            std::fs::write(format!("{out}/results.ppm"), ppm).unwrap();
        }
        // OK at (67, 174) dialog units into the centred window
        let (ox, oy, _, _) = hud.details_place(800, 500);
        assert!(hud.details_ok_at(800, 500, ox + 67 * 7 / 4 + 10, oy + 174 * 13 / 8 + 5));
        assert_eq!(hud.tool_at(17, TOOLBAR_TOP + 15), Some(Tool::OpenMap));
        // every button has its texts from the executable, a short name and
        // a longer line; the overview's and grid's change with their state
        if hud.strings.is_some() {
            for &(t, ..) in &TIPS {
                let (short, long) = hud.tip(t, true, false).unwrap();
                assert!(!short.is_empty() && long.len() >= short.len(), "{t:?}");
            }
            assert_ne!(hud.tip(Tool::Overview, true, false), hud.tip(Tool::Overview, false, false));
            assert_ne!(hud.tip(Tool::HexGrid, true, false), hud.tip(Tool::HexGrid, true, true));
            *hud.tooltip.borrow_mut() = Some((hud.tip(Tool::AboutFace, true, false).unwrap().0, (147, TOOLBAR_TOP + 15)));
            hud.pressed.set(Some((147, TOOLBAR_TOP + 15)));
            let c = hud.canvas(640, 200, |_| true, ["", "", ""], None, None, None, None, &[], None, None, None, None);
            if let Ok(out) = std::env::var("GBOH_SHOTS") {
                let pal = gboh_formats::pal::Palette::parse(&std::fs::read(Path::new(&dir).join("GAME.PAL")).unwrap()).unwrap();
                crate::ui::write_ppm(&c, &pal, &Path::new(&out).join("tooltip.ppm")).unwrap();
            }
        }
        assert_eq!(hud.tool_at(502, TOOLBAR_TOP + 15), Some(Tool::Undo));
        assert!(hud.overview_at("Issus", 800, 800 - 176 - 4 + 30 + 5, TOP + 4 + 26 + 5).is_some());
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(Path::new(&dir).join("GAME.PAL")).unwrap()).unwrap();
            let rgba = c.to_rgba(&pal);
            let mut ppm = format!("P6 {} {} 255\n", c.width, c.height).into_bytes();
            for p in rgba.chunks(4) {
                ppm.extend_from_slice(&p[..3]);
            }
            std::fs::write(format!("{out}/hud.ppm"), ppm).unwrap();
        }
    }
}
