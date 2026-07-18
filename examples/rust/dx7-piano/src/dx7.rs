/* ------------------------------------------------------------
Code generated with Faust Rust backend (faust-rs)
Compilation options: -lang rust
------------------------------------------------------------ */


#[allow(non_upper_case_globals, dead_code)]
static iTbl59: [i32; 20] = [0i32, 5i32, 9i32, 13i32, 17i32, 20i32, 23i32, 25i32, 27i32, 29i32, 31i32, 33i32, 35i32, 37i32, 39i32, 41i32, 42i32, 43i32, 45i32, 46i32];
#[allow(non_upper_case_globals, dead_code)]
static iTbl242: [i32; 64] = [0i32, 70i32, 86i32, 97i32, 106i32, 114i32, 121i32, 126i32, 132i32, 138i32, 142i32, 148i32, 152i32, 156i32, 160i32, 163i32, 166i32, 170i32, 173i32, 174i32, 178i32, 181i32, 184i32, 186i32, 189i32, 190i32, 194i32, 196i32, 198i32, 200i32, 202i32, 205i32, 206i32, 209i32, 211i32, 214i32, 216i32, 218i32, 220i32, 222i32, 224i32, 225i32, 227i32, 229i32, 230i32, 232i32, 233i32, 235i32, 237i32, 238i32, 240i32, 241i32, 242i32, 243i32, 244i32, 246i32, 246i32, 248i32, 249i32, 250i32, 251i32, 252i32, 253i32, 254i32];
#[allow(non_upper_case_globals, dead_code)]
static iTbl129: [i32; 33] = [0i32, 1i32, 2i32, 3i32, 4i32, 5i32, 6i32, 7i32, 8i32, 9i32, 11i32, 14i32, 16i32, 19i32, 23i32, 27i32, 33i32, 39i32, 47i32, 56i32, 66i32, 80i32, 94i32, 110i32, 126i32, 142i32, 158i32, 174i32, 190i32, 206i32, 222i32, 238i32, 250i32];
#[allow(non_upper_case_globals, dead_code)]
static iTbl382: [i32; 77] = [1764000i32, 1764000i32, 1411200i32, 1411200i32, 1190700i32, 1014300i32, 992250i32, 882000i32, 705600i32, 705600i32, 584325i32, 507150i32, 502740i32, 441000i32, 418950i32, 352800i32, 308700i32, 286650i32, 253575i32, 220500i32, 220500i32, 176400i32, 145530i32, 145530i32, 125685i32, 110250i32, 110250i32, 88200i32, 88200i32, 74970i32, 61740i32, 61740i32, 55125i32, 48510i32, 44100i32, 37485i32, 31311i32, 30870i32, 27562i32, 27562i32, 22050i32, 18522i32, 17640i32, 15435i32, 14112i32, 13230i32, 11025i32, 9261i32, 9261i32, 7717i32, 6615i32, 6615i32, 5512i32, 5512i32, 4410i32, 3969i32, 3969i32, 3439i32, 2866i32, 2690i32, 2249i32, 1984i32, 1896i32, 1808i32, 1411i32, 1367i32, 1234i32, 1146i32, 926i32, 837i32, 837i32, 705i32, 573i32, 573i32, 529i32, 441i32, 441i32];
#[allow(non_upper_case_globals, dead_code)]
static iTbl1047: [i32; 100] = [-128i32, -116i32, -104i32, -95i32, -85i32, -76i32, -68i32, -61i32, -56i32, -52i32, -49i32, -46i32, -43i32, -41i32, -39i32, -37i32, -35i32, -33i32, -32i32, -31i32, -30i32, -29i32, -28i32, -27i32, -26i32, -25i32, -24i32, -23i32, -22i32, -21i32, -20i32, -19i32, -18i32, -17i32, -16i32, -15i32, -14i32, -13i32, -12i32, -11i32, -10i32, -9i32, -8i32, -7i32, -6i32, -5i32, -4i32, -3i32, -2i32, -1i32, 0i32, 1i32, 2i32, 3i32, 4i32, 5i32, 6i32, 7i32, 8i32, 9i32, 10i32, 11i32, 12i32, 13i32, 14i32, 15i32, 16i32, 17i32, 18i32, 19i32, 20i32, 21i32, 22i32, 23i32, 24i32, 25i32, 26i32, 27i32, 28i32, 29i32, 30i32, 31i32, 32i32, 33i32, 34i32, 35i32, 38i32, 40i32, 43i32, 46i32, 49i32, 53i32, 58i32, 65i32, 73i32, 82i32, 92i32, 103i32, 115i32, 127i32];
#[allow(non_upper_case_globals, dead_code)]
static iTbl1102: [i32; 100] = [1i32, 2i32, 3i32, 3i32, 4i32, 4i32, 5i32, 5i32, 6i32, 6i32, 7i32, 7i32, 8i32, 8i32, 9i32, 9i32, 10i32, 10i32, 11i32, 11i32, 12i32, 12i32, 13i32, 13i32, 14i32, 14i32, 15i32, 16i32, 16i32, 17i32, 18i32, 18i32, 19i32, 20i32, 21i32, 22i32, 23i32, 24i32, 25i32, 26i32, 27i32, 28i32, 30i32, 31i32, 33i32, 34i32, 36i32, 37i32, 38i32, 39i32, 41i32, 42i32, 44i32, 46i32, 47i32, 49i32, 51i32, 53i32, 54i32, 56i32, 58i32, 60i32, 62i32, 64i32, 66i32, 68i32, 70i32, 72i32, 74i32, 76i32, 79i32, 82i32, 85i32, 88i32, 91i32, 94i32, 98i32, 102i32, 106i32, 110i32, 115i32, 120i32, 125i32, 130i32, 135i32, 141i32, 147i32, 153i32, 159i32, 165i32, 171i32, 178i32, 185i32, 193i32, 202i32, 211i32, 232i32, 243i32, 254i32, 255i32];
#[allow(non_upper_case_globals, dead_code)]
static iTbl734: [i32; 4] = [0i32, 4342338i32, 7171437i32, 16777216i32];
#[allow(non_upper_case_globals, dead_code)]
static iTbl938: [i32; 32] = [-16777216i32, 0i32, 16777216i32, 26591258i32, 33554432i32, 38955489i32, 43368474i32, 47099600i32, 50331648i32, 53182516i32, 55732705i32, 58039632i32, 60145690i32, 62083076i32, 63876816i32, 65546747i32, 67108864i32, 68576247i32, 69959732i32, 71268397i32, 72509921i32, 73690858i32, 74816848i32, 75892776i32, 76922906i32, 77910978i32, 78860292i32, 79773775i32, 80654032i32, 81503396i32, 82323963i32, 83117622i32];
#[allow(non_upper_case_globals, dead_code)]
static iTbl1202: [i32; 8] = [0i32, 10i32, 20i32, 33i32, 55i32, 92i32, 153i32, 255i32];

#[repr(C)]
pub struct Dx7Piano {
    fSampleRate: i32,
    fVec12: [f32; 2],
    fButton19: FaustFloat,
    fHslider42: FaustFloat,
    fHslider43: FaustFloat,
    fHslider51: FaustFloat,
    fHslider53: FaustFloat,
    fHslider54: FaustFloat,
    fHslider32: FaustFloat,
    fHslider33: FaustFloat,
    fHslider36: FaustFloat,
    fHslider37: FaustFloat,
    fHslider1: FaustFloat,
    fHslider2: FaustFloat,
    fHslider12: FaustFloat,
    fHslider15: FaustFloat,
    fHslider16: FaustFloat,
    fHslider84: FaustFloat,
    fHslider85: FaustFloat,
    fHslider93: FaustFloat,
    fHslider95: FaustFloat,
    fHslider96: FaustFloat,
    fHslider63: FaustFloat,
    fHslider64: FaustFloat,
    fHslider72: FaustFloat,
    fHslider74: FaustFloat,
    fHslider75: FaustFloat,
    fHslider127: FaustFloat,
    fHslider128: FaustFloat,
    fHslider136: FaustFloat,
    fHslider138: FaustFloat,
    fHslider139: FaustFloat,
    fHslider105: FaustFloat,
    fHslider106: FaustFloat,
    fHslider114: FaustFloat,
    fHslider116: FaustFloat,
    fHslider117: FaustFloat,
    fHslider45: FaustFloat,
    fEntry47: FaustFloat,
    fEntry49: FaustFloat,
    fHslider57: FaustFloat,
    fHslider22: FaustFloat,
    fEntry25: FaustFloat,
    fHslider26: FaustFloat,
    fCheckbox58: FaustFloat,
    fHslider60: FaustFloat,
    fHslider34: FaustFloat,
    fHslider40: FaustFloat,
    fHslider4: FaustFloat,
    fEntry8: FaustFloat,
    fEntry10: FaustFloat,
    fHslider20: FaustFloat,
    fCheckbox27: FaustFloat,
    fHslider29: FaustFloat,
    fHslider87: FaustFloat,
    fEntry89: FaustFloat,
    fEntry91: FaustFloat,
    fHslider99: FaustFloat,
    fCheckbox100: FaustFloat,
    fHslider102: FaustFloat,
    fHslider66: FaustFloat,
    fEntry68: FaustFloat,
    fEntry70: FaustFloat,
    fHslider78: FaustFloat,
    fCheckbox79: FaustFloat,
    fHslider81: FaustFloat,
    fHslider130: FaustFloat,
    fEntry132: FaustFloat,
    fEntry134: FaustFloat,
    fHslider142: FaustFloat,
    fCheckbox143: FaustFloat,
    fHslider145: FaustFloat,
    fHslider108: FaustFloat,
    fEntry110: FaustFloat,
    fEntry112: FaustFloat,
    fHslider120: FaustFloat,
    fCheckbox121: FaustFloat,
    fHslider123: FaustFloat,
    fHslider41: FaustFloat,
    fHslider48: FaustFloat,
    fHslider50: FaustFloat,
    fHslider13: FaustFloat,
    fHslider52: FaustFloat,
    fHslider56: FaustFloat,
    fHslider6: FaustFloat,
    fHslider5: FaustFloat,
    fHslider44: FaustFloat,
    fHslider55: FaustFloat,
    fConst1: f32,
    fConst2: f32,
    fHslider23: FaustFloat,
    fHslider24: FaustFloat,
    fConst3: f32,
    fHslider21: FaustFloat,
    fHslider61: FaustFloat,
    fHslider59: FaustFloat,
    fHslider31: FaustFloat,
    fHslider38: FaustFloat,
    fConst4: f32,
    fHslider35: FaustFloat,
    fHslider39: FaustFloat,
    fHslider0: FaustFloat,
    fHslider9: FaustFloat,
    fHslider11: FaustFloat,
    fHslider14: FaustFloat,
    fHslider18: FaustFloat,
    fHslider3: FaustFloat,
    fHslider17: FaustFloat,
    fHslider30: FaustFloat,
    fHslider28: FaustFloat,
    fHslider83: FaustFloat,
    fHslider90: FaustFloat,
    fHslider92: FaustFloat,
    fHslider94: FaustFloat,
    fHslider98: FaustFloat,
    fHslider86: FaustFloat,
    fHslider97: FaustFloat,
    fHslider103: FaustFloat,
    fHslider101: FaustFloat,
    fHslider62: FaustFloat,
    fHslider69: FaustFloat,
    fHslider71: FaustFloat,
    fHslider73: FaustFloat,
    fHslider77: FaustFloat,
    fHslider65: FaustFloat,
    fHslider76: FaustFloat,
    fHslider82: FaustFloat,
    fHslider80: FaustFloat,
    fHslider126: FaustFloat,
    fHslider133: FaustFloat,
    fHslider135: FaustFloat,
    fHslider137: FaustFloat,
    fHslider141: FaustFloat,
    fHslider129: FaustFloat,
    fHslider140: FaustFloat,
    fHslider146: FaustFloat,
    fHslider144: FaustFloat,
    fHslider104: FaustFloat,
    fHslider111: FaustFloat,
    fHslider113: FaustFloat,
    fHslider115: FaustFloat,
    fHslider119: FaustFloat,
    fHslider107: FaustFloat,
    fHslider118: FaustFloat,
    fHslider124: FaustFloat,
    fHslider122: FaustFloat,
    fHslider125: FaustFloat,
    fHslider46: FaustFloat,
    fHslider7: FaustFloat,
    fHslider88: FaustFloat,
    fHslider67: FaustFloat,
    fHslider131: FaustFloat,
    fHslider109: FaustFloat,
    iRec15971: [i32; 2],
    iRec15971_1: [i32; 2],
    iRec15971_2: [i32; 2],
    iRec15971_3: [i32; 2],
    iRec15971_4: [i32; 2],
    iRec15971_5: [i32; 2],
    iRec15971_6: [i32; 2],
    fRec16024: [f32; 2],
    fRec16024_1: [f32; 2],
    fRec16024_2: [f32; 2],
    fRec16024_3: [f32; 2],
    iRec16024_4: [i32; 2],
    fRec16024_5: [f32; 2],
    fRec16024_6: [f32; 2],
    fRec16090: [f32; 2],
    iRec16090_1: [i32; 2],
    iRec16090_2: [i32; 2],
    iRec16090_3: [i32; 2],
    fRec16090_4: [f32; 2],
    iRec16090_5: [i32; 2],
    iRec16339: [i32; 2],
    iRec16339_1: [i32; 2],
    iRec16339_2: [i32; 2],
    iRec16339_3: [i32; 2],
    iRec16339_4: [i32; 2],
    iRec16339_5: [i32; 2],
    iRec16339_6: [i32; 2],
    iRec16599: [i32; 2],
    iRec16599_1: [i32; 2],
    iRec16599_2: [i32; 2],
    iRec16599_3: [i32; 2],
    iRec16599_4: [i32; 2],
    iRec16599_5: [i32; 2],
    iRec16599_6: [i32; 2],
    iRec16851: [i32; 2],
    iRec16851_1: [i32; 2],
    iRec16851_2: [i32; 2],
    iRec16851_3: [i32; 2],
    iRec16851_4: [i32; 2],
    iRec16851_5: [i32; 2],
    iRec16851_6: [i32; 2],
    iRec17112: [i32; 2],
    iRec17112_1: [i32; 2],
    iRec17112_2: [i32; 2],
    iRec17112_3: [i32; 2],
    iRec17112_4: [i32; 2],
    iRec17112_5: [i32; 2],
    iRec17112_6: [i32; 2],
    iRec17364: [i32; 2],
    iRec17364_1: [i32; 2],
    iRec17364_2: [i32; 2],
    iRec17364_3: [i32; 2],
    iRec17364_4: [i32; 2],
    iRec17364_5: [i32; 2],
    iRec17364_6: [i32; 2],
    fRec17426: f32,
    fRec17443: f32,
    fRec17468: f32,
    fRec17485: f32,
    fRec17511: f32,
    fRec17528: f32,
    fRec17536: f32,
}

pub const FAUST_INPUTS: usize = 0;
pub const FAUST_OUTPUTS: usize = 2;
pub const FAUST_ACTIVES: usize = 147;
pub const FAUST_PASSIVES: usize = 0;

impl Dx7Piano {
    pub fn new() -> Dx7Piano {
        Dx7Piano {
            fSampleRate: 0,
            fVec12: [0.0f32; 2],
            fButton19: 0.0 as FaustFloat,
            fHslider42: 0.0 as FaustFloat,
            fHslider43: 0.0 as FaustFloat,
            fHslider51: 0.0 as FaustFloat,
            fHslider53: 0.0 as FaustFloat,
            fHslider54: 0.0 as FaustFloat,
            fHslider32: 0.0 as FaustFloat,
            fHslider33: 0.0 as FaustFloat,
            fHslider36: 0.0 as FaustFloat,
            fHslider37: 0.0 as FaustFloat,
            fHslider1: 0.0 as FaustFloat,
            fHslider2: 0.0 as FaustFloat,
            fHslider12: 0.0 as FaustFloat,
            fHslider15: 0.0 as FaustFloat,
            fHslider16: 0.0 as FaustFloat,
            fHslider84: 0.0 as FaustFloat,
            fHslider85: 0.0 as FaustFloat,
            fHslider93: 0.0 as FaustFloat,
            fHslider95: 0.0 as FaustFloat,
            fHslider96: 0.0 as FaustFloat,
            fHslider63: 0.0 as FaustFloat,
            fHslider64: 0.0 as FaustFloat,
            fHslider72: 0.0 as FaustFloat,
            fHslider74: 0.0 as FaustFloat,
            fHslider75: 0.0 as FaustFloat,
            fHslider127: 0.0 as FaustFloat,
            fHslider128: 0.0 as FaustFloat,
            fHslider136: 0.0 as FaustFloat,
            fHslider138: 0.0 as FaustFloat,
            fHslider139: 0.0 as FaustFloat,
            fHslider105: 0.0 as FaustFloat,
            fHslider106: 0.0 as FaustFloat,
            fHslider114: 0.0 as FaustFloat,
            fHslider116: 0.0 as FaustFloat,
            fHslider117: 0.0 as FaustFloat,
            fHslider45: 0.0 as FaustFloat,
            fEntry47: 0.0 as FaustFloat,
            fEntry49: 0.0 as FaustFloat,
            fHslider57: 0.0 as FaustFloat,
            fHslider22: 0.0 as FaustFloat,
            fEntry25: 0.0 as FaustFloat,
            fHslider26: 0.0 as FaustFloat,
            fCheckbox58: 0.0 as FaustFloat,
            fHslider60: 0.0 as FaustFloat,
            fHslider34: 0.0 as FaustFloat,
            fHslider40: 0.0 as FaustFloat,
            fHslider4: 0.0 as FaustFloat,
            fEntry8: 0.0 as FaustFloat,
            fEntry10: 0.0 as FaustFloat,
            fHslider20: 0.0 as FaustFloat,
            fCheckbox27: 0.0 as FaustFloat,
            fHslider29: 0.0 as FaustFloat,
            fHslider87: 0.0 as FaustFloat,
            fEntry89: 0.0 as FaustFloat,
            fEntry91: 0.0 as FaustFloat,
            fHslider99: 0.0 as FaustFloat,
            fCheckbox100: 0.0 as FaustFloat,
            fHslider102: 0.0 as FaustFloat,
            fHslider66: 0.0 as FaustFloat,
            fEntry68: 0.0 as FaustFloat,
            fEntry70: 0.0 as FaustFloat,
            fHslider78: 0.0 as FaustFloat,
            fCheckbox79: 0.0 as FaustFloat,
            fHslider81: 0.0 as FaustFloat,
            fHslider130: 0.0 as FaustFloat,
            fEntry132: 0.0 as FaustFloat,
            fEntry134: 0.0 as FaustFloat,
            fHslider142: 0.0 as FaustFloat,
            fCheckbox143: 0.0 as FaustFloat,
            fHslider145: 0.0 as FaustFloat,
            fHslider108: 0.0 as FaustFloat,
            fEntry110: 0.0 as FaustFloat,
            fEntry112: 0.0 as FaustFloat,
            fHslider120: 0.0 as FaustFloat,
            fCheckbox121: 0.0 as FaustFloat,
            fHslider123: 0.0 as FaustFloat,
            fHslider41: 0.0 as FaustFloat,
            fHslider48: 0.0 as FaustFloat,
            fHslider50: 0.0 as FaustFloat,
            fHslider13: 0.0 as FaustFloat,
            fHslider52: 0.0 as FaustFloat,
            fHslider56: 0.0 as FaustFloat,
            fHslider6: 0.0 as FaustFloat,
            fHslider5: 0.0 as FaustFloat,
            fHslider44: 0.0 as FaustFloat,
            fHslider55: 0.0 as FaustFloat,
            fConst1: 0.0f32,
            fConst2: 0.0f32,
            fHslider23: 0.0 as FaustFloat,
            fHslider24: 0.0 as FaustFloat,
            fConst3: 0.0f32,
            fHslider21: 0.0 as FaustFloat,
            fHslider61: 0.0 as FaustFloat,
            fHslider59: 0.0 as FaustFloat,
            fHslider31: 0.0 as FaustFloat,
            fHslider38: 0.0 as FaustFloat,
            fConst4: 0.0f32,
            fHslider35: 0.0 as FaustFloat,
            fHslider39: 0.0 as FaustFloat,
            fHslider0: 0.0 as FaustFloat,
            fHslider9: 0.0 as FaustFloat,
            fHslider11: 0.0 as FaustFloat,
            fHslider14: 0.0 as FaustFloat,
            fHslider18: 0.0 as FaustFloat,
            fHslider3: 0.0 as FaustFloat,
            fHslider17: 0.0 as FaustFloat,
            fHslider30: 0.0 as FaustFloat,
            fHslider28: 0.0 as FaustFloat,
            fHslider83: 0.0 as FaustFloat,
            fHslider90: 0.0 as FaustFloat,
            fHslider92: 0.0 as FaustFloat,
            fHslider94: 0.0 as FaustFloat,
            fHslider98: 0.0 as FaustFloat,
            fHslider86: 0.0 as FaustFloat,
            fHslider97: 0.0 as FaustFloat,
            fHslider103: 0.0 as FaustFloat,
            fHslider101: 0.0 as FaustFloat,
            fHslider62: 0.0 as FaustFloat,
            fHslider69: 0.0 as FaustFloat,
            fHslider71: 0.0 as FaustFloat,
            fHslider73: 0.0 as FaustFloat,
            fHslider77: 0.0 as FaustFloat,
            fHslider65: 0.0 as FaustFloat,
            fHslider76: 0.0 as FaustFloat,
            fHslider82: 0.0 as FaustFloat,
            fHslider80: 0.0 as FaustFloat,
            fHslider126: 0.0 as FaustFloat,
            fHslider133: 0.0 as FaustFloat,
            fHslider135: 0.0 as FaustFloat,
            fHslider137: 0.0 as FaustFloat,
            fHslider141: 0.0 as FaustFloat,
            fHslider129: 0.0 as FaustFloat,
            fHslider140: 0.0 as FaustFloat,
            fHslider146: 0.0 as FaustFloat,
            fHslider144: 0.0 as FaustFloat,
            fHslider104: 0.0 as FaustFloat,
            fHslider111: 0.0 as FaustFloat,
            fHslider113: 0.0 as FaustFloat,
            fHslider115: 0.0 as FaustFloat,
            fHslider119: 0.0 as FaustFloat,
            fHslider107: 0.0 as FaustFloat,
            fHslider118: 0.0 as FaustFloat,
            fHslider124: 0.0 as FaustFloat,
            fHslider122: 0.0 as FaustFloat,
            fHslider125: 0.0 as FaustFloat,
            fHslider46: 0.0 as FaustFloat,
            fHslider7: 0.0 as FaustFloat,
            fHslider88: 0.0 as FaustFloat,
            fHslider67: 0.0 as FaustFloat,
            fHslider131: 0.0 as FaustFloat,
            fHslider109: 0.0 as FaustFloat,
            iRec15971: [0; 2],
            iRec15971_1: [0; 2],
            iRec15971_2: [0; 2],
            iRec15971_3: [0; 2],
            iRec15971_4: [0; 2],
            iRec15971_5: [0; 2],
            iRec15971_6: [0; 2],
            fRec16024: [0.0f32; 2],
            fRec16024_1: [0.0f32; 2],
            fRec16024_2: [0.0f32; 2],
            fRec16024_3: [0.0f32; 2],
            iRec16024_4: [0; 2],
            fRec16024_5: [0.0f32; 2],
            fRec16024_6: [0.0f32; 2],
            fRec16090: [0.0f32; 2],
            iRec16090_1: [0; 2],
            iRec16090_2: [0; 2],
            iRec16090_3: [0; 2],
            fRec16090_4: [0.0f32; 2],
            iRec16090_5: [0; 2],
            iRec16339: [0; 2],
            iRec16339_1: [0; 2],
            iRec16339_2: [0; 2],
            iRec16339_3: [0; 2],
            iRec16339_4: [0; 2],
            iRec16339_5: [0; 2],
            iRec16339_6: [0; 2],
            iRec16599: [0; 2],
            iRec16599_1: [0; 2],
            iRec16599_2: [0; 2],
            iRec16599_3: [0; 2],
            iRec16599_4: [0; 2],
            iRec16599_5: [0; 2],
            iRec16599_6: [0; 2],
            iRec16851: [0; 2],
            iRec16851_1: [0; 2],
            iRec16851_2: [0; 2],
            iRec16851_3: [0; 2],
            iRec16851_4: [0; 2],
            iRec16851_5: [0; 2],
            iRec16851_6: [0; 2],
            iRec17112: [0; 2],
            iRec17112_1: [0; 2],
            iRec17112_2: [0; 2],
            iRec17112_3: [0; 2],
            iRec17112_4: [0; 2],
            iRec17112_5: [0; 2],
            iRec17112_6: [0; 2],
            iRec17364: [0; 2],
            iRec17364_1: [0; 2],
            iRec17364_2: [0; 2],
            iRec17364_3: [0; 2],
            iRec17364_4: [0; 2],
            iRec17364_5: [0; 2],
            iRec17364_6: [0; 2],
            fRec17426: 0.0f32,
            fRec17443: 0.0f32,
            fRec17468: 0.0f32,
            fRec17485: 0.0f32,
            fRec17511: 0.0f32,
            fRec17528: 0.0f32,
            fRec17536: 0.0f32,
        }
    }

    pub fn metadata(&self, m: &mut dyn Meta) {
    }

    pub fn get_sample_rate(&self) -> i32 {
        self.fSampleRate
    }

    pub fn get_num_inputs(&self) -> i32 {
        FAUST_INPUTS as i32
    }

    pub fn get_num_outputs(&self) -> i32 {
        FAUST_OUTPUTS as i32
    }

    pub fn class_init(sample_rate: i32) {
    }

    pub fn instance_constants(&mut self, sample_rate: i32) {
        self.fSampleRate = sample_rate;
        let mut fConst0: f32 = f32::min(192000.0f32, f32::max(1.0f32, ((self.fSampleRate) as f32)));
        self.fConst1 = (44100.0f32 / fConst0);
        self.fConst2 = (1.0f32 / fConst0);
        self.fConst3 = (0.005865102633833885f32 / fConst0);
        self.fConst4 = (1.502347469329834f32 / fConst0);
    }

    pub fn instance_reset_params(&mut self) {
        self.fButton19 = ((0.0f32) as FaustFloat);
        self.fHslider42 = ((99.0f32) as FaustFloat);
        self.fHslider43 = ((99.0f32) as FaustFloat);
        self.fHslider51 = ((0.0f32) as FaustFloat);
        self.fHslider53 = ((99.0f32) as FaustFloat);
        self.fHslider54 = ((99.0f32) as FaustFloat);
        self.fHslider32 = ((50.0f32) as FaustFloat);
        self.fHslider33 = ((50.0f32) as FaustFloat);
        self.fHslider36 = ((99.0f32) as FaustFloat);
        self.fHslider37 = ((99.0f32) as FaustFloat);
        self.fHslider1 = ((99.0f32) as FaustFloat);
        self.fHslider2 = ((99.0f32) as FaustFloat);
        self.fHslider12 = ((0.0f32) as FaustFloat);
        self.fHslider15 = ((99.0f32) as FaustFloat);
        self.fHslider16 = ((99.0f32) as FaustFloat);
        self.fHslider84 = ((99.0f32) as FaustFloat);
        self.fHslider85 = ((99.0f32) as FaustFloat);
        self.fHslider93 = ((0.0f32) as FaustFloat);
        self.fHslider95 = ((99.0f32) as FaustFloat);
        self.fHslider96 = ((99.0f32) as FaustFloat);
        self.fHslider63 = ((99.0f32) as FaustFloat);
        self.fHslider64 = ((99.0f32) as FaustFloat);
        self.fHslider72 = ((0.0f32) as FaustFloat);
        self.fHslider74 = ((99.0f32) as FaustFloat);
        self.fHslider75 = ((99.0f32) as FaustFloat);
        self.fHslider127 = ((99.0f32) as FaustFloat);
        self.fHslider128 = ((99.0f32) as FaustFloat);
        self.fHslider136 = ((0.0f32) as FaustFloat);
        self.fHslider138 = ((99.0f32) as FaustFloat);
        self.fHslider139 = ((99.0f32) as FaustFloat);
        self.fHslider105 = ((99.0f32) as FaustFloat);
        self.fHslider106 = ((99.0f32) as FaustFloat);
        self.fHslider114 = ((0.0f32) as FaustFloat);
        self.fHslider116 = ((99.0f32) as FaustFloat);
        self.fHslider117 = ((99.0f32) as FaustFloat);
        self.fHslider45 = ((99.0f32) as FaustFloat);
        self.fEntry47 = ((0.0f32) as FaustFloat);
        self.fEntry49 = ((0.0f32) as FaustFloat);
        self.fHslider57 = ((0.0f32) as FaustFloat);
        self.fHslider22 = ((1.0f32) as FaustFloat);
        self.fEntry25 = ((0.0f32) as FaustFloat);
        self.fHslider26 = ((1.0f32) as FaustFloat);
        self.fCheckbox58 = ((0.0f32) as FaustFloat);
        self.fHslider60 = ((1.0f32) as FaustFloat);
        self.fHslider34 = ((50.0f32) as FaustFloat);
        self.fHslider40 = ((3.0f32) as FaustFloat);
        self.fHslider4 = ((0.0f32) as FaustFloat);
        self.fEntry8 = ((0.0f32) as FaustFloat);
        self.fEntry10 = ((0.0f32) as FaustFloat);
        self.fHslider20 = ((0.0f32) as FaustFloat);
        self.fCheckbox27 = ((0.0f32) as FaustFloat);
        self.fHslider29 = ((1.0f32) as FaustFloat);
        self.fHslider87 = ((0.0f32) as FaustFloat);
        self.fEntry89 = ((0.0f32) as FaustFloat);
        self.fEntry91 = ((0.0f32) as FaustFloat);
        self.fHslider99 = ((0.0f32) as FaustFloat);
        self.fCheckbox100 = ((0.0f32) as FaustFloat);
        self.fHslider102 = ((1.0f32) as FaustFloat);
        self.fHslider66 = ((0.0f32) as FaustFloat);
        self.fEntry68 = ((0.0f32) as FaustFloat);
        self.fEntry70 = ((0.0f32) as FaustFloat);
        self.fHslider78 = ((0.0f32) as FaustFloat);
        self.fCheckbox79 = ((0.0f32) as FaustFloat);
        self.fHslider81 = ((1.0f32) as FaustFloat);
        self.fHslider130 = ((0.0f32) as FaustFloat);
        self.fEntry132 = ((0.0f32) as FaustFloat);
        self.fEntry134 = ((0.0f32) as FaustFloat);
        self.fHslider142 = ((0.0f32) as FaustFloat);
        self.fCheckbox143 = ((0.0f32) as FaustFloat);
        self.fHslider145 = ((1.0f32) as FaustFloat);
        self.fHslider108 = ((0.0f32) as FaustFloat);
        self.fEntry110 = ((0.0f32) as FaustFloat);
        self.fEntry112 = ((0.0f32) as FaustFloat);
        self.fHslider120 = ((0.0f32) as FaustFloat);
        self.fCheckbox121 = ((0.0f32) as FaustFloat);
        self.fHslider123 = ((1.0f32) as FaustFloat);
        self.fHslider41 = ((99.0f32) as FaustFloat);
        self.fHslider48 = ((0.0f32) as FaustFloat);
        self.fHslider50 = ((0.0f32) as FaustFloat);
        self.fHslider13 = ((0.800000011920929f32) as FaustFloat);
        self.fHslider52 = ((99.0f32) as FaustFloat);
        self.fHslider56 = ((0.0f32) as FaustFloat);
        self.fHslider6 = ((0.0f32) as FaustFloat);
        self.fHslider5 = ((400.0f32) as FaustFloat);
        self.fHslider44 = ((0.0f32) as FaustFloat);
        self.fHslider55 = ((99.0f32) as FaustFloat);
        self.fHslider23 = ((35.0f32) as FaustFloat);
        self.fHslider24 = ((0.0f32) as FaustFloat);
        self.fHslider21 = ((0.0f32) as FaustFloat);
        self.fHslider61 = ((0.0f32) as FaustFloat);
        self.fHslider59 = ((0.0f32) as FaustFloat);
        self.fHslider31 = ((50.0f32) as FaustFloat);
        self.fHslider38 = ((99.0f32) as FaustFloat);
        self.fHslider35 = ((99.0f32) as FaustFloat);
        self.fHslider39 = ((0.0f32) as FaustFloat);
        self.fHslider0 = ((99.0f32) as FaustFloat);
        self.fHslider9 = ((0.0f32) as FaustFloat);
        self.fHslider11 = ((0.0f32) as FaustFloat);
        self.fHslider14 = ((99.0f32) as FaustFloat);
        self.fHslider18 = ((0.0f32) as FaustFloat);
        self.fHslider3 = ((0.0f32) as FaustFloat);
        self.fHslider17 = ((99.0f32) as FaustFloat);
        self.fHslider30 = ((0.0f32) as FaustFloat);
        self.fHslider28 = ((0.0f32) as FaustFloat);
        self.fHslider83 = ((99.0f32) as FaustFloat);
        self.fHslider90 = ((0.0f32) as FaustFloat);
        self.fHslider92 = ((0.0f32) as FaustFloat);
        self.fHslider94 = ((99.0f32) as FaustFloat);
        self.fHslider98 = ((0.0f32) as FaustFloat);
        self.fHslider86 = ((0.0f32) as FaustFloat);
        self.fHslider97 = ((99.0f32) as FaustFloat);
        self.fHslider103 = ((0.0f32) as FaustFloat);
        self.fHslider101 = ((0.0f32) as FaustFloat);
        self.fHslider62 = ((99.0f32) as FaustFloat);
        self.fHslider69 = ((0.0f32) as FaustFloat);
        self.fHslider71 = ((0.0f32) as FaustFloat);
        self.fHslider73 = ((99.0f32) as FaustFloat);
        self.fHslider77 = ((0.0f32) as FaustFloat);
        self.fHslider65 = ((0.0f32) as FaustFloat);
        self.fHslider76 = ((99.0f32) as FaustFloat);
        self.fHslider82 = ((0.0f32) as FaustFloat);
        self.fHslider80 = ((0.0f32) as FaustFloat);
        self.fHslider126 = ((99.0f32) as FaustFloat);
        self.fHslider133 = ((0.0f32) as FaustFloat);
        self.fHslider135 = ((0.0f32) as FaustFloat);
        self.fHslider137 = ((99.0f32) as FaustFloat);
        self.fHslider141 = ((0.0f32) as FaustFloat);
        self.fHslider129 = ((0.0f32) as FaustFloat);
        self.fHslider140 = ((99.0f32) as FaustFloat);
        self.fHslider146 = ((0.0f32) as FaustFloat);
        self.fHslider144 = ((0.0f32) as FaustFloat);
        self.fHslider104 = ((99.0f32) as FaustFloat);
        self.fHslider111 = ((0.0f32) as FaustFloat);
        self.fHslider113 = ((0.0f32) as FaustFloat);
        self.fHslider115 = ((99.0f32) as FaustFloat);
        self.fHslider119 = ((0.0f32) as FaustFloat);
        self.fHslider107 = ((0.0f32) as FaustFloat);
        self.fHslider118 = ((99.0f32) as FaustFloat);
        self.fHslider124 = ((0.0f32) as FaustFloat);
        self.fHslider122 = ((0.0f32) as FaustFloat);
        self.fHslider125 = ((0.0f32) as FaustFloat);
        self.fHslider46 = ((0.0f32) as FaustFloat);
        self.fHslider7 = ((0.0f32) as FaustFloat);
        self.fHslider88 = ((0.0f32) as FaustFloat);
        self.fHslider67 = ((0.0f32) as FaustFloat);
        self.fHslider131 = ((0.0f32) as FaustFloat);
        self.fHslider109 = ((0.0f32) as FaustFloat);
    }

    pub fn instance_clear(&mut self) {
        for lDelay0 in 0..2i32 {
            self.fVec12[(lDelay0) as usize] = 0.0f32;
        }
        for lRec1 in 0..2i32 {
            self.iRec15971[(lRec1) as usize] = 0i32;
        }
        for lRec2 in 0..2i32 {
            self.iRec15971_1[(lRec2) as usize] = 0i32;
        }
        for lRec3 in 0..2i32 {
            self.iRec15971_2[(lRec3) as usize] = 0i32;
        }
        for lRec4 in 0..2i32 {
            self.iRec15971_3[(lRec4) as usize] = 0i32;
        }
        for lRec5 in 0..2i32 {
            self.iRec15971_4[(lRec5) as usize] = 0i32;
        }
        for lRec6 in 0..2i32 {
            self.iRec15971_5[(lRec6) as usize] = 0i32;
        }
        for lRec7 in 0..2i32 {
            self.iRec15971_6[(lRec7) as usize] = 0i32;
        }
        for lRec8 in 0..2i32 {
            self.fRec16024[(lRec8) as usize] = 0.0f32;
        }
        for lRec9 in 0..2i32 {
            self.fRec16024_1[(lRec9) as usize] = 0.0f32;
        }
        for lRec10 in 0..2i32 {
            self.fRec16024_2[(lRec10) as usize] = 0.0f32;
        }
        for lRec11 in 0..2i32 {
            self.fRec16024_3[(lRec11) as usize] = 0.0f32;
        }
        for lRec12 in 0..2i32 {
            self.iRec16024_4[(lRec12) as usize] = 0i32;
        }
        for lRec13 in 0..2i32 {
            self.fRec16024_5[(lRec13) as usize] = 0.0f32;
        }
        for lRec14 in 0..2i32 {
            self.fRec16024_6[(lRec14) as usize] = 0.0f32;
        }
        for lRec15 in 0..2i32 {
            self.fRec16090[(lRec15) as usize] = 0.0f32;
        }
        for lRec16 in 0..2i32 {
            self.iRec16090_1[(lRec16) as usize] = 0i32;
        }
        for lRec17 in 0..2i32 {
            self.iRec16090_2[(lRec17) as usize] = 0i32;
        }
        for lRec18 in 0..2i32 {
            self.iRec16090_3[(lRec18) as usize] = 0i32;
        }
        for lRec19 in 0..2i32 {
            self.fRec16090_4[(lRec19) as usize] = 0.0f32;
        }
        for lRec20 in 0..2i32 {
            self.iRec16090_5[(lRec20) as usize] = 0i32;
        }
        for lRec21 in 0..2i32 {
            self.iRec16339[(lRec21) as usize] = 0i32;
        }
        for lRec22 in 0..2i32 {
            self.iRec16339_1[(lRec22) as usize] = 0i32;
        }
        for lRec23 in 0..2i32 {
            self.iRec16339_2[(lRec23) as usize] = 0i32;
        }
        for lRec24 in 0..2i32 {
            self.iRec16339_3[(lRec24) as usize] = 0i32;
        }
        for lRec25 in 0..2i32 {
            self.iRec16339_4[(lRec25) as usize] = 0i32;
        }
        for lRec26 in 0..2i32 {
            self.iRec16339_5[(lRec26) as usize] = 0i32;
        }
        for lRec27 in 0..2i32 {
            self.iRec16339_6[(lRec27) as usize] = 0i32;
        }
        for lRec28 in 0..2i32 {
            self.iRec16599[(lRec28) as usize] = 0i32;
        }
        for lRec29 in 0..2i32 {
            self.iRec16599_1[(lRec29) as usize] = 0i32;
        }
        for lRec30 in 0..2i32 {
            self.iRec16599_2[(lRec30) as usize] = 0i32;
        }
        for lRec31 in 0..2i32 {
            self.iRec16599_3[(lRec31) as usize] = 0i32;
        }
        for lRec32 in 0..2i32 {
            self.iRec16599_4[(lRec32) as usize] = 0i32;
        }
        for lRec33 in 0..2i32 {
            self.iRec16599_5[(lRec33) as usize] = 0i32;
        }
        for lRec34 in 0..2i32 {
            self.iRec16599_6[(lRec34) as usize] = 0i32;
        }
        for lRec35 in 0..2i32 {
            self.iRec16851[(lRec35) as usize] = 0i32;
        }
        for lRec36 in 0..2i32 {
            self.iRec16851_1[(lRec36) as usize] = 0i32;
        }
        for lRec37 in 0..2i32 {
            self.iRec16851_2[(lRec37) as usize] = 0i32;
        }
        for lRec38 in 0..2i32 {
            self.iRec16851_3[(lRec38) as usize] = 0i32;
        }
        for lRec39 in 0..2i32 {
            self.iRec16851_4[(lRec39) as usize] = 0i32;
        }
        for lRec40 in 0..2i32 {
            self.iRec16851_5[(lRec40) as usize] = 0i32;
        }
        for lRec41 in 0..2i32 {
            self.iRec16851_6[(lRec41) as usize] = 0i32;
        }
        for lRec42 in 0..2i32 {
            self.iRec17112[(lRec42) as usize] = 0i32;
        }
        for lRec43 in 0..2i32 {
            self.iRec17112_1[(lRec43) as usize] = 0i32;
        }
        for lRec44 in 0..2i32 {
            self.iRec17112_2[(lRec44) as usize] = 0i32;
        }
        for lRec45 in 0..2i32 {
            self.iRec17112_3[(lRec45) as usize] = 0i32;
        }
        for lRec46 in 0..2i32 {
            self.iRec17112_4[(lRec46) as usize] = 0i32;
        }
        for lRec47 in 0..2i32 {
            self.iRec17112_5[(lRec47) as usize] = 0i32;
        }
        for lRec48 in 0..2i32 {
            self.iRec17112_6[(lRec48) as usize] = 0i32;
        }
        for lRec49 in 0..2i32 {
            self.iRec17364[(lRec49) as usize] = 0i32;
        }
        for lRec50 in 0..2i32 {
            self.iRec17364_1[(lRec50) as usize] = 0i32;
        }
        for lRec51 in 0..2i32 {
            self.iRec17364_2[(lRec51) as usize] = 0i32;
        }
        for lRec52 in 0..2i32 {
            self.iRec17364_3[(lRec52) as usize] = 0i32;
        }
        for lRec53 in 0..2i32 {
            self.iRec17364_4[(lRec53) as usize] = 0i32;
        }
        for lRec54 in 0..2i32 {
            self.iRec17364_5[(lRec54) as usize] = 0i32;
        }
        for lRec55 in 0..2i32 {
            self.iRec17364_6[(lRec55) as usize] = 0i32;
        }
        self.fRec17426 = 0.0f32;
        self.fRec17443 = 0.0f32;
        self.fRec17468 = 0.0f32;
        self.fRec17485 = 0.0f32;
        self.fRec17511 = 0.0f32;
        self.fRec17528 = 0.0f32;
        self.fRec17536 = 0.0f32;
    }

    pub fn instance_init(&mut self, sample_rate: i32) {
        self.instance_constants(sample_rate);
        self.instance_reset_params();
        self.instance_clear();
    }

    pub fn init(&mut self, sample_rate: i32) {
        Self::class_init(sample_rate);
        self.instance_init(sample_rate);
    }

    pub fn build_user_interface(&self, ui_interface: &mut dyn UI<FaustFloat>) {
        Self::build_user_interface_static(ui_interface);
    }

    pub fn build_user_interface_static(ui_interface: &mut dyn UI<FaustFloat>) {
        ui_interface.open_horizontal_box("DX7");
        ui_interface.open_vertical_box("Global");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Main");
        ui_interface.declare(Some(ParamIndex(0)), "0", "");
        ui_interface.declare(Some(ParamIndex(0)), "style", "knob");
        ui_interface.add_horizontal_slider("Feedback", ParamIndex(0), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(1)), "1", "");
        ui_interface.declare(Some(ParamIndex(1)), "style", "knob");
        ui_interface.add_horizontal_slider("Transpose", ParamIndex(1), 0.0 as FaustFloat, -24.0 as FaustFloat, 24.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(2)), "2", "");
        ui_interface.declare(Some(ParamIndex(2)), "style", "knob");
        ui_interface.add_horizontal_slider("Osc Key Sync", ParamIndex(2), 1.0 as FaustFloat, 0.0 as FaustFloat, 1.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_horizontal_box("Pitch EG Levels");
        ui_interface.declare(Some(ParamIndex(3)), "0", "");
        ui_interface.declare(Some(ParamIndex(3)), "style", "knob");
        ui_interface.add_horizontal_slider("L1", ParamIndex(3), 50.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(4)), "1", "");
        ui_interface.declare(Some(ParamIndex(4)), "style", "knob");
        ui_interface.add_horizontal_slider("L2", ParamIndex(4), 50.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(5)), "2", "");
        ui_interface.declare(Some(ParamIndex(5)), "style", "knob");
        ui_interface.add_horizontal_slider("L3", ParamIndex(5), 50.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(6)), "3", "");
        ui_interface.declare(Some(ParamIndex(6)), "style", "knob");
        ui_interface.add_horizontal_slider("L4", ParamIndex(6), 50.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "2", "");
        ui_interface.open_horizontal_box("Pitch EG Rates");
        ui_interface.declare(Some(ParamIndex(7)), "0", "");
        ui_interface.declare(Some(ParamIndex(7)), "style", "knob");
        ui_interface.add_horizontal_slider("R1", ParamIndex(7), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(8)), "1", "");
        ui_interface.declare(Some(ParamIndex(8)), "style", "knob");
        ui_interface.add_horizontal_slider("R2", ParamIndex(8), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(9)), "2", "");
        ui_interface.declare(Some(ParamIndex(9)), "style", "knob");
        ui_interface.add_horizontal_slider("R3", ParamIndex(9), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(10)), "3", "");
        ui_interface.declare(Some(ParamIndex(10)), "style", "knob");
        ui_interface.add_horizontal_slider("R4", ParamIndex(10), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "3", "");
        ui_interface.open_horizontal_box("LFO");
        ui_interface.declare(Some(ParamIndex(11)), "0", "");
        ui_interface.declare(Some(ParamIndex(11)), "style", "menu{'Triangle':0;'Saw Down':1;'Saw Up':2;'Square':3;'Sine':4;'Sample & Hold':5}");
        ui_interface.add_num_entry("Wave", ParamIndex(11), 0.0 as FaustFloat, 0.0 as FaustFloat, 5.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(12)), "1", "");
        ui_interface.declare(Some(ParamIndex(12)), "style", "knob");
        ui_interface.add_horizontal_slider("Speed", ParamIndex(12), 35.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(13)), "2", "");
        ui_interface.declare(Some(ParamIndex(13)), "style", "knob");
        ui_interface.add_horizontal_slider("Delay", ParamIndex(13), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(14)), "3", "");
        ui_interface.declare(Some(ParamIndex(14)), "style", "knob");
        ui_interface.add_horizontal_slider("PMD", ParamIndex(14), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(15)), "4", "");
        ui_interface.declare(Some(ParamIndex(15)), "style", "knob");
        ui_interface.add_horizontal_slider("AMD", ParamIndex(15), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(16)), "5", "");
        ui_interface.declare(Some(ParamIndex(16)), "style", "knob");
        ui_interface.add_horizontal_slider("Sync", ParamIndex(16), 1.0 as FaustFloat, 0.0 as FaustFloat, 1.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(17)), "6", "");
        ui_interface.declare(Some(ParamIndex(17)), "style", "knob");
        ui_interface.add_horizontal_slider("P Mod Sens", ParamIndex(17), 3.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "0", "");
        ui_interface.open_vertical_box("Operator 1");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Tone");
        ui_interface.declare(Some(ParamIndex(18)), "0", "");
        ui_interface.declare(Some(ParamIndex(18)), "style", "knob");
        ui_interface.add_horizontal_slider("Tune", ParamIndex(18), 0.0 as FaustFloat, -7.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(19)), "1", "");
        ui_interface.declare(Some(ParamIndex(19)), "style", "knob");
        ui_interface.add_horizontal_slider("Coarse", ParamIndex(19), 1.0 as FaustFloat, 0.0 as FaustFloat, 31.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(20)), "2", "");
        ui_interface.declare(Some(ParamIndex(20)), "style", "knob");
        ui_interface.add_horizontal_slider("Fine", ParamIndex(20), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(21)), "3", "");
        ui_interface.declare(Some(ParamIndex(21)), "style", "knob");
        ui_interface.add_check_button("Freq Mode", ParamIndex(21));
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_vertical_box("Amp Env Generator");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Levels");
        ui_interface.declare(Some(ParamIndex(22)), "0", "");
        ui_interface.declare(Some(ParamIndex(22)), "style", "knob");
        ui_interface.add_horizontal_slider("L1", ParamIndex(22), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(23)), "1", "");
        ui_interface.declare(Some(ParamIndex(23)), "style", "knob");
        ui_interface.add_horizontal_slider("L2", ParamIndex(23), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(24)), "2", "");
        ui_interface.declare(Some(ParamIndex(24)), "style", "knob");
        ui_interface.add_horizontal_slider("L3", ParamIndex(24), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(25)), "3", "");
        ui_interface.declare(Some(ParamIndex(25)), "style", "knob");
        ui_interface.add_horizontal_slider("L4", ParamIndex(25), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_horizontal_box("Rates");
        ui_interface.declare(Some(ParamIndex(26)), "0", "");
        ui_interface.declare(Some(ParamIndex(26)), "style", "knob");
        ui_interface.add_horizontal_slider("R1", ParamIndex(26), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(27)), "1", "");
        ui_interface.declare(Some(ParamIndex(27)), "style", "knob");
        ui_interface.add_horizontal_slider("R2", ParamIndex(27), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(28)), "2", "");
        ui_interface.declare(Some(ParamIndex(28)), "style", "knob");
        ui_interface.add_horizontal_slider("R3", ParamIndex(28), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(29)), "3", "");
        ui_interface.declare(Some(ParamIndex(29)), "style", "knob");
        ui_interface.add_horizontal_slider("R4", ParamIndex(29), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "2", "");
        ui_interface.open_horizontal_box("Level");
        ui_interface.declare(Some(ParamIndex(30)), "0", "");
        ui_interface.declare(Some(ParamIndex(30)), "style", "knob");
        ui_interface.add_horizontal_slider("Level", ParamIndex(30), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(31)), "1", "");
        ui_interface.declare(Some(ParamIndex(31)), "style", "knob");
        ui_interface.add_horizontal_slider("Key Vel", ParamIndex(31), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(32)), "2", "");
        ui_interface.declare(Some(ParamIndex(32)), "style", "knob");
        ui_interface.add_horizontal_slider("A Mod Sens", ParamIndex(32), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(33)), "3", "");
        ui_interface.declare(Some(ParamIndex(33)), "style", "knob");
        ui_interface.add_horizontal_slider("Rate Scaling", ParamIndex(33), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "3", "");
        ui_interface.open_horizontal_box("Breakpoint");
        ui_interface.declare(Some(ParamIndex(34)), "0", "");
        ui_interface.declare(Some(ParamIndex(34)), "style", "knob");
        ui_interface.add_horizontal_slider("Breakpoint", ParamIndex(34), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(35)), "1", "");
        ui_interface.declare(Some(ParamIndex(35)), "style", "knob");
        ui_interface.add_horizontal_slider("L Depth", ParamIndex(35), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(36)), "2", "");
        ui_interface.declare(Some(ParamIndex(36)), "style", "knob");
        ui_interface.add_horizontal_slider("R Depth", ParamIndex(36), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(37)), "3", "");
        ui_interface.declare(Some(ParamIndex(37)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("L Curve", ParamIndex(37), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(38)), "4", "");
        ui_interface.declare(Some(ParamIndex(38)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("R Curve", ParamIndex(38), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_vertical_box("Operator 2");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Tone");
        ui_interface.declare(Some(ParamIndex(39)), "0", "");
        ui_interface.declare(Some(ParamIndex(39)), "style", "knob");
        ui_interface.add_horizontal_slider("Tune", ParamIndex(39), 0.0 as FaustFloat, -7.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(40)), "1", "");
        ui_interface.declare(Some(ParamIndex(40)), "style", "knob");
        ui_interface.add_horizontal_slider("Coarse", ParamIndex(40), 1.0 as FaustFloat, 0.0 as FaustFloat, 31.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(41)), "2", "");
        ui_interface.declare(Some(ParamIndex(41)), "style", "knob");
        ui_interface.add_horizontal_slider("Fine", ParamIndex(41), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(42)), "3", "");
        ui_interface.declare(Some(ParamIndex(42)), "style", "knob");
        ui_interface.add_check_button("Freq Mode", ParamIndex(42));
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_vertical_box("Amp Env Generator");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Levels");
        ui_interface.declare(Some(ParamIndex(43)), "0", "");
        ui_interface.declare(Some(ParamIndex(43)), "style", "knob");
        ui_interface.add_horizontal_slider("L1", ParamIndex(43), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(44)), "1", "");
        ui_interface.declare(Some(ParamIndex(44)), "style", "knob");
        ui_interface.add_horizontal_slider("L2", ParamIndex(44), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(45)), "2", "");
        ui_interface.declare(Some(ParamIndex(45)), "style", "knob");
        ui_interface.add_horizontal_slider("L3", ParamIndex(45), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(46)), "3", "");
        ui_interface.declare(Some(ParamIndex(46)), "style", "knob");
        ui_interface.add_horizontal_slider("L4", ParamIndex(46), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_horizontal_box("Rates");
        ui_interface.declare(Some(ParamIndex(47)), "0", "");
        ui_interface.declare(Some(ParamIndex(47)), "style", "knob");
        ui_interface.add_horizontal_slider("R1", ParamIndex(47), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(48)), "1", "");
        ui_interface.declare(Some(ParamIndex(48)), "style", "knob");
        ui_interface.add_horizontal_slider("R2", ParamIndex(48), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(49)), "2", "");
        ui_interface.declare(Some(ParamIndex(49)), "style", "knob");
        ui_interface.add_horizontal_slider("R3", ParamIndex(49), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(50)), "3", "");
        ui_interface.declare(Some(ParamIndex(50)), "style", "knob");
        ui_interface.add_horizontal_slider("R4", ParamIndex(50), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "2", "");
        ui_interface.open_horizontal_box("Level");
        ui_interface.declare(Some(ParamIndex(51)), "0", "");
        ui_interface.declare(Some(ParamIndex(51)), "style", "knob");
        ui_interface.add_horizontal_slider("Level", ParamIndex(51), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(52)), "1", "");
        ui_interface.declare(Some(ParamIndex(52)), "style", "knob");
        ui_interface.add_horizontal_slider("Key Vel", ParamIndex(52), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(53)), "2", "");
        ui_interface.declare(Some(ParamIndex(53)), "style", "knob");
        ui_interface.add_horizontal_slider("A Mod Sens", ParamIndex(53), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(54)), "3", "");
        ui_interface.declare(Some(ParamIndex(54)), "style", "knob");
        ui_interface.add_horizontal_slider("Rate Scaling", ParamIndex(54), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "3", "");
        ui_interface.open_horizontal_box("Breakpoint");
        ui_interface.declare(Some(ParamIndex(55)), "0", "");
        ui_interface.declare(Some(ParamIndex(55)), "style", "knob");
        ui_interface.add_horizontal_slider("Breakpoint", ParamIndex(55), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(56)), "1", "");
        ui_interface.declare(Some(ParamIndex(56)), "style", "knob");
        ui_interface.add_horizontal_slider("L Depth", ParamIndex(56), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(57)), "2", "");
        ui_interface.declare(Some(ParamIndex(57)), "style", "knob");
        ui_interface.add_horizontal_slider("R Depth", ParamIndex(57), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(58)), "3", "");
        ui_interface.declare(Some(ParamIndex(58)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("L Curve", ParamIndex(58), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(59)), "4", "");
        ui_interface.declare(Some(ParamIndex(59)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("R Curve", ParamIndex(59), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "2", "");
        ui_interface.open_vertical_box("Operator 3");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Tone");
        ui_interface.declare(Some(ParamIndex(60)), "0", "");
        ui_interface.declare(Some(ParamIndex(60)), "style", "knob");
        ui_interface.add_horizontal_slider("Tune", ParamIndex(60), 0.0 as FaustFloat, -7.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(61)), "1", "");
        ui_interface.declare(Some(ParamIndex(61)), "style", "knob");
        ui_interface.add_horizontal_slider("Coarse", ParamIndex(61), 1.0 as FaustFloat, 0.0 as FaustFloat, 31.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(62)), "2", "");
        ui_interface.declare(Some(ParamIndex(62)), "style", "knob");
        ui_interface.add_horizontal_slider("Fine", ParamIndex(62), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(63)), "3", "");
        ui_interface.declare(Some(ParamIndex(63)), "style", "knob");
        ui_interface.add_check_button("Freq Mode", ParamIndex(63));
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_vertical_box("Amp Env Generator");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Levels");
        ui_interface.declare(Some(ParamIndex(64)), "0", "");
        ui_interface.declare(Some(ParamIndex(64)), "style", "knob");
        ui_interface.add_horizontal_slider("L1", ParamIndex(64), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(65)), "1", "");
        ui_interface.declare(Some(ParamIndex(65)), "style", "knob");
        ui_interface.add_horizontal_slider("L2", ParamIndex(65), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(66)), "2", "");
        ui_interface.declare(Some(ParamIndex(66)), "style", "knob");
        ui_interface.add_horizontal_slider("L3", ParamIndex(66), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(67)), "3", "");
        ui_interface.declare(Some(ParamIndex(67)), "style", "knob");
        ui_interface.add_horizontal_slider("L4", ParamIndex(67), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_horizontal_box("Rates");
        ui_interface.declare(Some(ParamIndex(68)), "0", "");
        ui_interface.declare(Some(ParamIndex(68)), "style", "knob");
        ui_interface.add_horizontal_slider("R1", ParamIndex(68), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(69)), "1", "");
        ui_interface.declare(Some(ParamIndex(69)), "style", "knob");
        ui_interface.add_horizontal_slider("R2", ParamIndex(69), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(70)), "2", "");
        ui_interface.declare(Some(ParamIndex(70)), "style", "knob");
        ui_interface.add_horizontal_slider("R3", ParamIndex(70), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(71)), "3", "");
        ui_interface.declare(Some(ParamIndex(71)), "style", "knob");
        ui_interface.add_horizontal_slider("R4", ParamIndex(71), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "2", "");
        ui_interface.open_horizontal_box("Level");
        ui_interface.declare(Some(ParamIndex(72)), "0", "");
        ui_interface.declare(Some(ParamIndex(72)), "style", "knob");
        ui_interface.add_horizontal_slider("Level", ParamIndex(72), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(73)), "1", "");
        ui_interface.declare(Some(ParamIndex(73)), "style", "knob");
        ui_interface.add_horizontal_slider("Key Vel", ParamIndex(73), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(74)), "2", "");
        ui_interface.declare(Some(ParamIndex(74)), "style", "knob");
        ui_interface.add_horizontal_slider("A Mod Sens", ParamIndex(74), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(75)), "3", "");
        ui_interface.declare(Some(ParamIndex(75)), "style", "knob");
        ui_interface.add_horizontal_slider("Rate Scaling", ParamIndex(75), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "3", "");
        ui_interface.open_horizontal_box("Breakpoint");
        ui_interface.declare(Some(ParamIndex(76)), "0", "");
        ui_interface.declare(Some(ParamIndex(76)), "style", "knob");
        ui_interface.add_horizontal_slider("Breakpoint", ParamIndex(76), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(77)), "1", "");
        ui_interface.declare(Some(ParamIndex(77)), "style", "knob");
        ui_interface.add_horizontal_slider("L Depth", ParamIndex(77), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(78)), "2", "");
        ui_interface.declare(Some(ParamIndex(78)), "style", "knob");
        ui_interface.add_horizontal_slider("R Depth", ParamIndex(78), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(79)), "3", "");
        ui_interface.declare(Some(ParamIndex(79)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("L Curve", ParamIndex(79), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(80)), "4", "");
        ui_interface.declare(Some(ParamIndex(80)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("R Curve", ParamIndex(80), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "3", "");
        ui_interface.open_vertical_box("Operator 4");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Tone");
        ui_interface.declare(Some(ParamIndex(81)), "0", "");
        ui_interface.declare(Some(ParamIndex(81)), "style", "knob");
        ui_interface.add_horizontal_slider("Tune", ParamIndex(81), 0.0 as FaustFloat, -7.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(82)), "1", "");
        ui_interface.declare(Some(ParamIndex(82)), "style", "knob");
        ui_interface.add_horizontal_slider("Coarse", ParamIndex(82), 1.0 as FaustFloat, 0.0 as FaustFloat, 31.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(83)), "2", "");
        ui_interface.declare(Some(ParamIndex(83)), "style", "knob");
        ui_interface.add_horizontal_slider("Fine", ParamIndex(83), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(84)), "3", "");
        ui_interface.declare(Some(ParamIndex(84)), "style", "knob");
        ui_interface.add_check_button("Freq Mode", ParamIndex(84));
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_vertical_box("Amp Env Generator");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Levels");
        ui_interface.declare(Some(ParamIndex(85)), "0", "");
        ui_interface.declare(Some(ParamIndex(85)), "style", "knob");
        ui_interface.add_horizontal_slider("L1", ParamIndex(85), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(86)), "1", "");
        ui_interface.declare(Some(ParamIndex(86)), "style", "knob");
        ui_interface.add_horizontal_slider("L2", ParamIndex(86), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(87)), "2", "");
        ui_interface.declare(Some(ParamIndex(87)), "style", "knob");
        ui_interface.add_horizontal_slider("L3", ParamIndex(87), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(88)), "3", "");
        ui_interface.declare(Some(ParamIndex(88)), "style", "knob");
        ui_interface.add_horizontal_slider("L4", ParamIndex(88), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_horizontal_box("Rates");
        ui_interface.declare(Some(ParamIndex(89)), "0", "");
        ui_interface.declare(Some(ParamIndex(89)), "style", "knob");
        ui_interface.add_horizontal_slider("R1", ParamIndex(89), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(90)), "1", "");
        ui_interface.declare(Some(ParamIndex(90)), "style", "knob");
        ui_interface.add_horizontal_slider("R2", ParamIndex(90), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(91)), "2", "");
        ui_interface.declare(Some(ParamIndex(91)), "style", "knob");
        ui_interface.add_horizontal_slider("R3", ParamIndex(91), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(92)), "3", "");
        ui_interface.declare(Some(ParamIndex(92)), "style", "knob");
        ui_interface.add_horizontal_slider("R4", ParamIndex(92), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "2", "");
        ui_interface.open_horizontal_box("Level");
        ui_interface.declare(Some(ParamIndex(93)), "0", "");
        ui_interface.declare(Some(ParamIndex(93)), "style", "knob");
        ui_interface.add_horizontal_slider("Level", ParamIndex(93), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(94)), "1", "");
        ui_interface.declare(Some(ParamIndex(94)), "style", "knob");
        ui_interface.add_horizontal_slider("Key Vel", ParamIndex(94), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(95)), "2", "");
        ui_interface.declare(Some(ParamIndex(95)), "style", "knob");
        ui_interface.add_horizontal_slider("A Mod Sens", ParamIndex(95), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(96)), "3", "");
        ui_interface.declare(Some(ParamIndex(96)), "style", "knob");
        ui_interface.add_horizontal_slider("Rate Scaling", ParamIndex(96), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "3", "");
        ui_interface.open_horizontal_box("Breakpoint");
        ui_interface.declare(Some(ParamIndex(97)), "0", "");
        ui_interface.declare(Some(ParamIndex(97)), "style", "knob");
        ui_interface.add_horizontal_slider("Breakpoint", ParamIndex(97), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(98)), "1", "");
        ui_interface.declare(Some(ParamIndex(98)), "style", "knob");
        ui_interface.add_horizontal_slider("L Depth", ParamIndex(98), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(99)), "2", "");
        ui_interface.declare(Some(ParamIndex(99)), "style", "knob");
        ui_interface.add_horizontal_slider("R Depth", ParamIndex(99), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(100)), "3", "");
        ui_interface.declare(Some(ParamIndex(100)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("L Curve", ParamIndex(100), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(101)), "4", "");
        ui_interface.declare(Some(ParamIndex(101)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("R Curve", ParamIndex(101), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "4", "");
        ui_interface.open_vertical_box("Operator 5");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Tone");
        ui_interface.declare(Some(ParamIndex(102)), "0", "");
        ui_interface.declare(Some(ParamIndex(102)), "style", "knob");
        ui_interface.add_horizontal_slider("Tune", ParamIndex(102), 0.0 as FaustFloat, -7.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(103)), "1", "");
        ui_interface.declare(Some(ParamIndex(103)), "style", "knob");
        ui_interface.add_horizontal_slider("Coarse", ParamIndex(103), 1.0 as FaustFloat, 0.0 as FaustFloat, 31.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(104)), "2", "");
        ui_interface.declare(Some(ParamIndex(104)), "style", "knob");
        ui_interface.add_horizontal_slider("Fine", ParamIndex(104), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(105)), "3", "");
        ui_interface.declare(Some(ParamIndex(105)), "style", "knob");
        ui_interface.add_check_button("Freq Mode", ParamIndex(105));
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_vertical_box("Amp Env Generator");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Levels");
        ui_interface.declare(Some(ParamIndex(106)), "0", "");
        ui_interface.declare(Some(ParamIndex(106)), "style", "knob");
        ui_interface.add_horizontal_slider("L1", ParamIndex(106), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(107)), "1", "");
        ui_interface.declare(Some(ParamIndex(107)), "style", "knob");
        ui_interface.add_horizontal_slider("L2", ParamIndex(107), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(108)), "2", "");
        ui_interface.declare(Some(ParamIndex(108)), "style", "knob");
        ui_interface.add_horizontal_slider("L3", ParamIndex(108), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(109)), "3", "");
        ui_interface.declare(Some(ParamIndex(109)), "style", "knob");
        ui_interface.add_horizontal_slider("L4", ParamIndex(109), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_horizontal_box("Rates");
        ui_interface.declare(Some(ParamIndex(110)), "0", "");
        ui_interface.declare(Some(ParamIndex(110)), "style", "knob");
        ui_interface.add_horizontal_slider("R1", ParamIndex(110), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(111)), "1", "");
        ui_interface.declare(Some(ParamIndex(111)), "style", "knob");
        ui_interface.add_horizontal_slider("R2", ParamIndex(111), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(112)), "2", "");
        ui_interface.declare(Some(ParamIndex(112)), "style", "knob");
        ui_interface.add_horizontal_slider("R3", ParamIndex(112), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(113)), "3", "");
        ui_interface.declare(Some(ParamIndex(113)), "style", "knob");
        ui_interface.add_horizontal_slider("R4", ParamIndex(113), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "2", "");
        ui_interface.open_horizontal_box("Level");
        ui_interface.declare(Some(ParamIndex(114)), "0", "");
        ui_interface.declare(Some(ParamIndex(114)), "style", "knob");
        ui_interface.add_horizontal_slider("Level", ParamIndex(114), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(115)), "1", "");
        ui_interface.declare(Some(ParamIndex(115)), "style", "knob");
        ui_interface.add_horizontal_slider("Key Vel", ParamIndex(115), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(116)), "2", "");
        ui_interface.declare(Some(ParamIndex(116)), "style", "knob");
        ui_interface.add_horizontal_slider("A Mod Sens", ParamIndex(116), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(117)), "3", "");
        ui_interface.declare(Some(ParamIndex(117)), "style", "knob");
        ui_interface.add_horizontal_slider("Rate Scaling", ParamIndex(117), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "3", "");
        ui_interface.open_horizontal_box("Breakpoint");
        ui_interface.declare(Some(ParamIndex(118)), "0", "");
        ui_interface.declare(Some(ParamIndex(118)), "style", "knob");
        ui_interface.add_horizontal_slider("Breakpoint", ParamIndex(118), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(119)), "1", "");
        ui_interface.declare(Some(ParamIndex(119)), "style", "knob");
        ui_interface.add_horizontal_slider("L Depth", ParamIndex(119), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(120)), "2", "");
        ui_interface.declare(Some(ParamIndex(120)), "style", "knob");
        ui_interface.add_horizontal_slider("R Depth", ParamIndex(120), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(121)), "3", "");
        ui_interface.declare(Some(ParamIndex(121)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("L Curve", ParamIndex(121), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(122)), "4", "");
        ui_interface.declare(Some(ParamIndex(122)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("R Curve", ParamIndex(122), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "5", "");
        ui_interface.open_vertical_box("Operator 6");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Tone");
        ui_interface.declare(Some(ParamIndex(123)), "0", "");
        ui_interface.declare(Some(ParamIndex(123)), "style", "knob");
        ui_interface.add_horizontal_slider("Tune", ParamIndex(123), 0.0 as FaustFloat, -7.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(124)), "1", "");
        ui_interface.declare(Some(ParamIndex(124)), "style", "knob");
        ui_interface.add_horizontal_slider("Coarse", ParamIndex(124), 1.0 as FaustFloat, 0.0 as FaustFloat, 31.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(125)), "2", "");
        ui_interface.declare(Some(ParamIndex(125)), "style", "knob");
        ui_interface.add_horizontal_slider("Fine", ParamIndex(125), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(126)), "3", "");
        ui_interface.declare(Some(ParamIndex(126)), "style", "knob");
        ui_interface.add_check_button("Freq Mode", ParamIndex(126));
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_vertical_box("Amp Env Generator");
        ui_interface.declare(None, "0", "");
        ui_interface.open_horizontal_box("Levels");
        ui_interface.declare(Some(ParamIndex(127)), "0", "");
        ui_interface.declare(Some(ParamIndex(127)), "style", "knob");
        ui_interface.add_horizontal_slider("L1", ParamIndex(127), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(128)), "1", "");
        ui_interface.declare(Some(ParamIndex(128)), "style", "knob");
        ui_interface.add_horizontal_slider("L2", ParamIndex(128), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(129)), "2", "");
        ui_interface.declare(Some(ParamIndex(129)), "style", "knob");
        ui_interface.add_horizontal_slider("L3", ParamIndex(129), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(130)), "3", "");
        ui_interface.declare(Some(ParamIndex(130)), "style", "knob");
        ui_interface.add_horizontal_slider("L4", ParamIndex(130), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "1", "");
        ui_interface.open_horizontal_box("Rates");
        ui_interface.declare(Some(ParamIndex(131)), "0", "");
        ui_interface.declare(Some(ParamIndex(131)), "style", "knob");
        ui_interface.add_horizontal_slider("R1", ParamIndex(131), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(132)), "1", "");
        ui_interface.declare(Some(ParamIndex(132)), "style", "knob");
        ui_interface.add_horizontal_slider("R2", ParamIndex(132), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(133)), "2", "");
        ui_interface.declare(Some(ParamIndex(133)), "style", "knob");
        ui_interface.add_horizontal_slider("R3", ParamIndex(133), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(134)), "3", "");
        ui_interface.declare(Some(ParamIndex(134)), "style", "knob");
        ui_interface.add_horizontal_slider("R4", ParamIndex(134), 99.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(None, "2", "");
        ui_interface.open_horizontal_box("Level");
        ui_interface.declare(Some(ParamIndex(135)), "0", "");
        ui_interface.declare(Some(ParamIndex(135)), "style", "knob");
        ui_interface.add_horizontal_slider("Level", ParamIndex(135), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(136)), "1", "");
        ui_interface.declare(Some(ParamIndex(136)), "style", "knob");
        ui_interface.add_horizontal_slider("Key Vel", ParamIndex(136), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(137)), "2", "");
        ui_interface.declare(Some(ParamIndex(137)), "style", "knob");
        ui_interface.add_horizontal_slider("A Mod Sens", ParamIndex(137), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(138)), "3", "");
        ui_interface.declare(Some(ParamIndex(138)), "style", "knob");
        ui_interface.add_horizontal_slider("Rate Scaling", ParamIndex(138), 0.0 as FaustFloat, 0.0 as FaustFloat, 7.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.declare(None, "3", "");
        ui_interface.open_horizontal_box("Breakpoint");
        ui_interface.declare(Some(ParamIndex(139)), "0", "");
        ui_interface.declare(Some(ParamIndex(139)), "style", "knob");
        ui_interface.add_horizontal_slider("Breakpoint", ParamIndex(139), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(140)), "1", "");
        ui_interface.declare(Some(ParamIndex(140)), "style", "knob");
        ui_interface.add_horizontal_slider("L Depth", ParamIndex(140), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(141)), "2", "");
        ui_interface.declare(Some(ParamIndex(141)), "style", "knob");
        ui_interface.add_horizontal_slider("R Depth", ParamIndex(141), 0.0 as FaustFloat, 0.0 as FaustFloat, 99.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(142)), "3", "");
        ui_interface.declare(Some(ParamIndex(142)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("L Curve", ParamIndex(142), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(143)), "4", "");
        ui_interface.declare(Some(ParamIndex(143)), "style", "menu{'-LIN':0;'-EXP':1;'+EXP':2;'+LIN':3}");
        ui_interface.add_num_entry("R Curve", ParamIndex(143), 0.0 as FaustFloat, 0.0 as FaustFloat, 3.0 as FaustFloat, 1.0 as FaustFloat);
        ui_interface.close_box();
        ui_interface.close_box();
        ui_interface.declare(Some(ParamIndex(144)), "hidden", "1");
        ui_interface.add_horizontal_slider("freq", ParamIndex(144), 400.0 as FaustFloat, 50.0 as FaustFloat, 1000.0 as FaustFloat, 0.01 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(145)), "hidden", "1");
        ui_interface.add_horizontal_slider("gain", ParamIndex(145), 0.8 as FaustFloat, 0.0 as FaustFloat, 1.0 as FaustFloat, 0.01 as FaustFloat);
        ui_interface.declare(Some(ParamIndex(146)), "hidden", "1");
        ui_interface.add_button("gate", ParamIndex(146));
        ui_interface.close_box();
    }

    pub fn get_param(&self, param: ParamIndex) -> Option<FaustFloat> {
        match param.0 {
            0 => Some(self.fHslider125),
            1 => Some(self.fHslider6),
            2 => Some(self.fHslider26),
            3 => Some(self.fHslider31),
            4 => Some(self.fHslider32),
            5 => Some(self.fHslider33),
            6 => Some(self.fHslider34),
            7 => Some(self.fHslider35),
            8 => Some(self.fHslider36),
            9 => Some(self.fHslider37),
            10 => Some(self.fHslider38),
            11 => Some(self.fEntry25),
            12 => Some(self.fHslider23),
            13 => Some(self.fHslider24),
            14 => Some(self.fHslider39),
            15 => Some(self.fHslider21),
            16 => Some(self.fHslider22),
            17 => Some(self.fHslider40),
            18 => Some(self.fHslider59),
            19 => Some(self.fHslider60),
            20 => Some(self.fHslider61),
            21 => Some(self.fCheckbox58),
            22 => Some(self.fHslider41),
            23 => Some(self.fHslider42),
            24 => Some(self.fHslider43),
            25 => Some(self.fHslider44),
            26 => Some(self.fHslider52),
            27 => Some(self.fHslider53),
            28 => Some(self.fHslider54),
            29 => Some(self.fHslider55),
            30 => Some(self.fHslider45),
            31 => Some(self.fHslider51),
            32 => Some(self.fHslider57),
            33 => Some(self.fHslider56),
            34 => Some(self.fHslider46),
            35 => Some(self.fHslider48),
            36 => Some(self.fHslider50),
            37 => Some(self.fEntry47),
            38 => Some(self.fEntry49),
            39 => Some(self.fHslider28),
            40 => Some(self.fHslider29),
            41 => Some(self.fHslider30),
            42 => Some(self.fCheckbox27),
            43 => Some(self.fHslider0),
            44 => Some(self.fHslider1),
            45 => Some(self.fHslider2),
            46 => Some(self.fHslider3),
            47 => Some(self.fHslider14),
            48 => Some(self.fHslider15),
            49 => Some(self.fHslider16),
            50 => Some(self.fHslider17),
            51 => Some(self.fHslider4),
            52 => Some(self.fHslider12),
            53 => Some(self.fHslider20),
            54 => Some(self.fHslider18),
            55 => Some(self.fHslider7),
            56 => Some(self.fHslider9),
            57 => Some(self.fHslider11),
            58 => Some(self.fEntry8),
            59 => Some(self.fEntry10),
            60 => Some(self.fHslider101),
            61 => Some(self.fHslider102),
            62 => Some(self.fHslider103),
            63 => Some(self.fCheckbox100),
            64 => Some(self.fHslider83),
            65 => Some(self.fHslider84),
            66 => Some(self.fHslider85),
            67 => Some(self.fHslider86),
            68 => Some(self.fHslider94),
            69 => Some(self.fHslider95),
            70 => Some(self.fHslider96),
            71 => Some(self.fHslider97),
            72 => Some(self.fHslider87),
            73 => Some(self.fHslider93),
            74 => Some(self.fHslider99),
            75 => Some(self.fHslider98),
            76 => Some(self.fHslider88),
            77 => Some(self.fHslider90),
            78 => Some(self.fHslider92),
            79 => Some(self.fEntry89),
            80 => Some(self.fEntry91),
            81 => Some(self.fHslider80),
            82 => Some(self.fHslider81),
            83 => Some(self.fHslider82),
            84 => Some(self.fCheckbox79),
            85 => Some(self.fHslider62),
            86 => Some(self.fHslider63),
            87 => Some(self.fHslider64),
            88 => Some(self.fHslider65),
            89 => Some(self.fHslider73),
            90 => Some(self.fHslider74),
            91 => Some(self.fHslider75),
            92 => Some(self.fHslider76),
            93 => Some(self.fHslider66),
            94 => Some(self.fHslider72),
            95 => Some(self.fHslider78),
            96 => Some(self.fHslider77),
            97 => Some(self.fHslider67),
            98 => Some(self.fHslider69),
            99 => Some(self.fHslider71),
            100 => Some(self.fEntry68),
            101 => Some(self.fEntry70),
            102 => Some(self.fHslider144),
            103 => Some(self.fHslider145),
            104 => Some(self.fHslider146),
            105 => Some(self.fCheckbox143),
            106 => Some(self.fHslider126),
            107 => Some(self.fHslider127),
            108 => Some(self.fHslider128),
            109 => Some(self.fHslider129),
            110 => Some(self.fHslider137),
            111 => Some(self.fHslider138),
            112 => Some(self.fHslider139),
            113 => Some(self.fHslider140),
            114 => Some(self.fHslider130),
            115 => Some(self.fHslider136),
            116 => Some(self.fHslider142),
            117 => Some(self.fHslider141),
            118 => Some(self.fHslider131),
            119 => Some(self.fHslider133),
            120 => Some(self.fHslider135),
            121 => Some(self.fEntry132),
            122 => Some(self.fEntry134),
            123 => Some(self.fHslider122),
            124 => Some(self.fHslider123),
            125 => Some(self.fHslider124),
            126 => Some(self.fCheckbox121),
            127 => Some(self.fHslider104),
            128 => Some(self.fHslider105),
            129 => Some(self.fHslider106),
            130 => Some(self.fHslider107),
            131 => Some(self.fHslider115),
            132 => Some(self.fHslider116),
            133 => Some(self.fHslider117),
            134 => Some(self.fHslider118),
            135 => Some(self.fHslider108),
            136 => Some(self.fHslider114),
            137 => Some(self.fHslider120),
            138 => Some(self.fHslider119),
            139 => Some(self.fHslider109),
            140 => Some(self.fHslider111),
            141 => Some(self.fHslider113),
            142 => Some(self.fEntry110),
            143 => Some(self.fEntry112),
            144 => Some(self.fHslider5),
            145 => Some(self.fHslider13),
            146 => Some(self.fButton19),
            _ => None,
        }
    }

    pub fn set_param(&mut self, param: ParamIndex, value: FaustFloat) {
        match param.0 {
            0 => { self.fHslider125 = value },
            1 => { self.fHslider6 = value },
            2 => { self.fHslider26 = value },
            3 => { self.fHslider31 = value },
            4 => { self.fHslider32 = value },
            5 => { self.fHslider33 = value },
            6 => { self.fHslider34 = value },
            7 => { self.fHslider35 = value },
            8 => { self.fHslider36 = value },
            9 => { self.fHslider37 = value },
            10 => { self.fHslider38 = value },
            11 => { self.fEntry25 = value },
            12 => { self.fHslider23 = value },
            13 => { self.fHslider24 = value },
            14 => { self.fHslider39 = value },
            15 => { self.fHslider21 = value },
            16 => { self.fHslider22 = value },
            17 => { self.fHslider40 = value },
            18 => { self.fHslider59 = value },
            19 => { self.fHslider60 = value },
            20 => { self.fHslider61 = value },
            21 => { self.fCheckbox58 = value },
            22 => { self.fHslider41 = value },
            23 => { self.fHslider42 = value },
            24 => { self.fHslider43 = value },
            25 => { self.fHslider44 = value },
            26 => { self.fHslider52 = value },
            27 => { self.fHslider53 = value },
            28 => { self.fHslider54 = value },
            29 => { self.fHslider55 = value },
            30 => { self.fHslider45 = value },
            31 => { self.fHslider51 = value },
            32 => { self.fHslider57 = value },
            33 => { self.fHslider56 = value },
            34 => { self.fHslider46 = value },
            35 => { self.fHslider48 = value },
            36 => { self.fHslider50 = value },
            37 => { self.fEntry47 = value },
            38 => { self.fEntry49 = value },
            39 => { self.fHslider28 = value },
            40 => { self.fHslider29 = value },
            41 => { self.fHslider30 = value },
            42 => { self.fCheckbox27 = value },
            43 => { self.fHslider0 = value },
            44 => { self.fHslider1 = value },
            45 => { self.fHslider2 = value },
            46 => { self.fHslider3 = value },
            47 => { self.fHslider14 = value },
            48 => { self.fHslider15 = value },
            49 => { self.fHslider16 = value },
            50 => { self.fHslider17 = value },
            51 => { self.fHslider4 = value },
            52 => { self.fHslider12 = value },
            53 => { self.fHslider20 = value },
            54 => { self.fHslider18 = value },
            55 => { self.fHslider7 = value },
            56 => { self.fHslider9 = value },
            57 => { self.fHslider11 = value },
            58 => { self.fEntry8 = value },
            59 => { self.fEntry10 = value },
            60 => { self.fHslider101 = value },
            61 => { self.fHslider102 = value },
            62 => { self.fHslider103 = value },
            63 => { self.fCheckbox100 = value },
            64 => { self.fHslider83 = value },
            65 => { self.fHslider84 = value },
            66 => { self.fHslider85 = value },
            67 => { self.fHslider86 = value },
            68 => { self.fHslider94 = value },
            69 => { self.fHslider95 = value },
            70 => { self.fHslider96 = value },
            71 => { self.fHslider97 = value },
            72 => { self.fHslider87 = value },
            73 => { self.fHslider93 = value },
            74 => { self.fHslider99 = value },
            75 => { self.fHslider98 = value },
            76 => { self.fHslider88 = value },
            77 => { self.fHslider90 = value },
            78 => { self.fHslider92 = value },
            79 => { self.fEntry89 = value },
            80 => { self.fEntry91 = value },
            81 => { self.fHslider80 = value },
            82 => { self.fHslider81 = value },
            83 => { self.fHslider82 = value },
            84 => { self.fCheckbox79 = value },
            85 => { self.fHslider62 = value },
            86 => { self.fHslider63 = value },
            87 => { self.fHslider64 = value },
            88 => { self.fHslider65 = value },
            89 => { self.fHslider73 = value },
            90 => { self.fHslider74 = value },
            91 => { self.fHslider75 = value },
            92 => { self.fHslider76 = value },
            93 => { self.fHslider66 = value },
            94 => { self.fHslider72 = value },
            95 => { self.fHslider78 = value },
            96 => { self.fHslider77 = value },
            97 => { self.fHslider67 = value },
            98 => { self.fHslider69 = value },
            99 => { self.fHslider71 = value },
            100 => { self.fEntry68 = value },
            101 => { self.fEntry70 = value },
            102 => { self.fHslider144 = value },
            103 => { self.fHslider145 = value },
            104 => { self.fHslider146 = value },
            105 => { self.fCheckbox143 = value },
            106 => { self.fHslider126 = value },
            107 => { self.fHslider127 = value },
            108 => { self.fHslider128 = value },
            109 => { self.fHslider129 = value },
            110 => { self.fHslider137 = value },
            111 => { self.fHslider138 = value },
            112 => { self.fHslider139 = value },
            113 => { self.fHslider140 = value },
            114 => { self.fHslider130 = value },
            115 => { self.fHslider136 = value },
            116 => { self.fHslider142 = value },
            117 => { self.fHslider141 = value },
            118 => { self.fHslider131 = value },
            119 => { self.fHslider133 = value },
            120 => { self.fHslider135 = value },
            121 => { self.fEntry132 = value },
            122 => { self.fEntry134 = value },
            123 => { self.fHslider122 = value },
            124 => { self.fHslider123 = value },
            125 => { self.fHslider124 = value },
            126 => { self.fCheckbox121 = value },
            127 => { self.fHslider104 = value },
            128 => { self.fHslider105 = value },
            129 => { self.fHslider106 = value },
            130 => { self.fHslider107 = value },
            131 => { self.fHslider115 = value },
            132 => { self.fHslider116 = value },
            133 => { self.fHslider117 = value },
            134 => { self.fHslider118 = value },
            135 => { self.fHslider108 = value },
            136 => { self.fHslider114 = value },
            137 => { self.fHslider120 = value },
            138 => { self.fHslider119 = value },
            139 => { self.fHslider109 = value },
            140 => { self.fHslider111 = value },
            141 => { self.fHslider113 = value },
            142 => { self.fEntry110 = value },
            143 => { self.fEntry112 = value },
            144 => { self.fHslider5 = value },
            145 => { self.fHslider13 = value },
            146 => { self.fButton19 = value },
            _ => {},
        }
    }

    pub fn compute(&mut self, count: usize, inputs: &[impl AsRef<[FaustFloat]>], outputs: &mut [impl AsMut<[FaustFloat]>]) {
        // signal_fir_fastlane_step2a: executable base slice
        // io: inputs=0 outputs=2
        // signals: 2
        let mut fSlow0: f32 = ((self.fButton19) as f32);
        let mut fSlow1: f32 = f32::round(((self.fHslider42) as f32));
        let mut fSlow2: f32 = f32::round(((self.fHslider43) as f32));
        let mut fSlow3: f32 = f32::round(((self.fHslider51) as f32));
        let mut fSlow4: f32 = f32::round(((self.fHslider53) as f32));
        let mut fSlow5: f32 = f32::round(((self.fHslider54) as f32));
        let mut fSlow6: f32 = f32::round(((self.fHslider32) as f32));
        let mut fSlow7: f32 = f32::round(((self.fHslider33) as f32));
        let mut fSlow8: f32 = f32::round(((self.fHslider36) as f32));
        let mut fSlow9: f32 = f32::round(((self.fHslider37) as f32));
        let mut fSlow10: f32 = f32::round(((self.fHslider1) as f32));
        let mut fSlow11: f32 = f32::round(((self.fHslider2) as f32));
        let mut fSlow12: f32 = f32::round(((self.fHslider12) as f32));
        let mut fSlow13: f32 = f32::round(((self.fHslider15) as f32));
        let mut fSlow14: f32 = f32::round(((self.fHslider16) as f32));
        let mut fSlow15: f32 = f32::round(((self.fHslider84) as f32));
        let mut fSlow16: f32 = f32::round(((self.fHslider85) as f32));
        let mut fSlow17: f32 = f32::round(((self.fHslider93) as f32));
        let mut fSlow18: f32 = f32::round(((self.fHslider95) as f32));
        let mut fSlow19: f32 = f32::round(((self.fHslider96) as f32));
        let mut fSlow20: f32 = f32::round(((self.fHslider63) as f32));
        let mut fSlow21: f32 = f32::round(((self.fHslider64) as f32));
        let mut fSlow22: f32 = f32::round(((self.fHslider72) as f32));
        let mut fSlow23: f32 = f32::round(((self.fHslider74) as f32));
        let mut fSlow24: f32 = f32::round(((self.fHslider75) as f32));
        let mut fSlow25: f32 = f32::round(((self.fHslider127) as f32));
        let mut fSlow26: f32 = f32::round(((self.fHslider128) as f32));
        let mut fSlow27: f32 = f32::round(((self.fHslider136) as f32));
        let mut fSlow28: f32 = f32::round(((self.fHslider138) as f32));
        let mut fSlow29: f32 = f32::round(((self.fHslider139) as f32));
        let mut fSlow30: f32 = f32::round(((self.fHslider105) as f32));
        let mut fSlow31: f32 = f32::round(((self.fHslider106) as f32));
        let mut fSlow32: f32 = f32::round(((self.fHslider114) as f32));
        let mut fSlow33: f32 = f32::round(((self.fHslider116) as f32));
        let mut fSlow34: f32 = f32::round(((self.fHslider117) as f32));
        let mut fSlow35: f32 = f32::round(((self.fHslider45) as f32));
        let mut iSlow0: i32 = (((fSlow35 >= 20.0f32)) as i32);
        let mut iSlow1: i32 = ((f32::round(fSlow35)) as i32);
        let mut fSlow36: f32 = f32::round(((self.fEntry47) as f32));
        let mut iSlow2: i32 = (((fSlow36 < 2.0f32)) as i32);
        let mut iSlow3: i32 = ((((fSlow36 == 0.0f32)) as i32) | (((fSlow36 == 3.0f32)) as i32));
        let mut fSlow37: f32 = f32::round(((self.fEntry49) as f32));
        let mut iSlow4: i32 = (((fSlow37 < 2.0f32)) as i32);
        let mut iSlow5: i32 = ((((fSlow37 == 0.0f32)) as i32) | (((fSlow37 == 3.0f32)) as i32));
        let mut iSlow6: i32 = ((f32::round(f32::round(((self.fHslider57) as f32)))) as i32);
        let mut iSlow7: i32 = ((f32::round(((self.fHslider22) as f32))) as i32);
        let mut fSlow38: f32 = f32::round(((self.fEntry25) as f32));
        let mut iSlow8: i32 = (((fSlow38 >= 3.0f32)) as i32);
        let mut iSlow9: i32 = (((fSlow38 >= 2.0f32)) as i32);
        let mut iSlow10: i32 = (((fSlow38 >= 1.0f32)) as i32);
        let mut iSlow11: i32 = (((fSlow38 >= 5.0f32)) as i32);
        let mut iSlow12: i32 = (((fSlow38 >= 4.0f32)) as i32);
        let mut iSlow13: i32 = ((f32::round(((self.fHslider26) as f32))) as i32);
        let mut iSlow14: i32 = ((f32::round(((self.fCheckbox58) as f32))) as i32);
        let mut iSlow15: i32 = ((f32::round(((self.fHslider60) as f32))) as i32);
        let mut iSlow16: i32 = ((f32::round((((iSlow15 & 31i32)) as f32))) as i32);
        let mut fSlow39: f32 = f32::round(((self.fHslider34) as f32));
        let mut iSlow17: i32 = ((f32::round(fSlow39)) as i32);
        let mut iSlow18: i32 = ((f32::round(f32::round(((self.fHslider40) as f32)))) as i32);
        let mut fSlow40: f32 = f32::round(((self.fHslider4) as f32));
        let mut iSlow19: i32 = (((fSlow40 >= 20.0f32)) as i32);
        let mut iSlow20: i32 = ((f32::round(fSlow40)) as i32);
        let mut fSlow41: f32 = f32::round(((self.fEntry8) as f32));
        let mut iSlow21: i32 = (((fSlow41 < 2.0f32)) as i32);
        let mut iSlow22: i32 = ((((fSlow41 == 0.0f32)) as i32) | (((fSlow41 == 3.0f32)) as i32));
        let mut fSlow42: f32 = f32::round(((self.fEntry10) as f32));
        let mut iSlow23: i32 = (((fSlow42 < 2.0f32)) as i32);
        let mut iSlow24: i32 = ((((fSlow42 == 0.0f32)) as i32) | (((fSlow42 == 3.0f32)) as i32));
        let mut iSlow25: i32 = ((f32::round(f32::round(((self.fHslider20) as f32)))) as i32);
        let mut iSlow26: i32 = ((f32::round(((self.fCheckbox27) as f32))) as i32);
        let mut iSlow27: i32 = ((f32::round(((self.fHslider29) as f32))) as i32);
        let mut iSlow28: i32 = ((f32::round((((iSlow27 & 31i32)) as f32))) as i32);
        let mut fSlow43: f32 = f32::round(((self.fHslider87) as f32));
        let mut iSlow29: i32 = (((fSlow43 >= 20.0f32)) as i32);
        let mut iSlow30: i32 = ((f32::round(fSlow43)) as i32);
        let mut fSlow44: f32 = f32::round(((self.fEntry89) as f32));
        let mut iSlow31: i32 = (((fSlow44 < 2.0f32)) as i32);
        let mut iSlow32: i32 = ((((fSlow44 == 0.0f32)) as i32) | (((fSlow44 == 3.0f32)) as i32));
        let mut fSlow45: f32 = f32::round(((self.fEntry91) as f32));
        let mut iSlow33: i32 = (((fSlow45 < 2.0f32)) as i32);
        let mut iSlow34: i32 = ((((fSlow45 == 0.0f32)) as i32) | (((fSlow45 == 3.0f32)) as i32));
        let mut iSlow35: i32 = ((f32::round(f32::round(((self.fHslider99) as f32)))) as i32);
        let mut iSlow36: i32 = ((f32::round(((self.fCheckbox100) as f32))) as i32);
        let mut iSlow37: i32 = ((f32::round(((self.fHslider102) as f32))) as i32);
        let mut iSlow38: i32 = ((f32::round((((iSlow37 & 31i32)) as f32))) as i32);
        let mut fSlow46: f32 = f32::round(((self.fHslider66) as f32));
        let mut iSlow39: i32 = (((fSlow46 >= 20.0f32)) as i32);
        let mut iSlow40: i32 = ((f32::round(fSlow46)) as i32);
        let mut fSlow47: f32 = f32::round(((self.fEntry68) as f32));
        let mut iSlow41: i32 = (((fSlow47 < 2.0f32)) as i32);
        let mut iSlow42: i32 = ((((fSlow47 == 0.0f32)) as i32) | (((fSlow47 == 3.0f32)) as i32));
        let mut fSlow48: f32 = f32::round(((self.fEntry70) as f32));
        let mut iSlow43: i32 = (((fSlow48 < 2.0f32)) as i32);
        let mut iSlow44: i32 = ((((fSlow48 == 0.0f32)) as i32) | (((fSlow48 == 3.0f32)) as i32));
        let mut iSlow45: i32 = ((f32::round(f32::round(((self.fHslider78) as f32)))) as i32);
        let mut iSlow46: i32 = ((f32::round(((self.fCheckbox79) as f32))) as i32);
        let mut iSlow47: i32 = ((f32::round(((self.fHslider81) as f32))) as i32);
        let mut iSlow48: i32 = ((f32::round((((iSlow47 & 31i32)) as f32))) as i32);
        let mut fSlow49: f32 = f32::round(((self.fHslider130) as f32));
        let mut iSlow49: i32 = (((fSlow49 >= 20.0f32)) as i32);
        let mut iSlow50: i32 = ((f32::round(fSlow49)) as i32);
        let mut fSlow50: f32 = f32::round(((self.fEntry132) as f32));
        let mut iSlow51: i32 = (((fSlow50 < 2.0f32)) as i32);
        let mut iSlow52: i32 = ((((fSlow50 == 0.0f32)) as i32) | (((fSlow50 == 3.0f32)) as i32));
        let mut fSlow51: f32 = f32::round(((self.fEntry134) as f32));
        let mut iSlow53: i32 = (((fSlow51 < 2.0f32)) as i32);
        let mut iSlow54: i32 = ((((fSlow51 == 0.0f32)) as i32) | (((fSlow51 == 3.0f32)) as i32));
        let mut iSlow55: i32 = ((f32::round(f32::round(((self.fHslider142) as f32)))) as i32);
        let mut iSlow56: i32 = ((f32::round(((self.fCheckbox143) as f32))) as i32);
        let mut iSlow57: i32 = ((f32::round(((self.fHslider145) as f32))) as i32);
        let mut iSlow58: i32 = ((f32::round((((iSlow57 & 31i32)) as f32))) as i32);
        let mut fSlow52: f32 = f32::round(((self.fHslider108) as f32));
        let mut iSlow59: i32 = (((fSlow52 >= 20.0f32)) as i32);
        let mut iSlow60: i32 = ((f32::round(fSlow52)) as i32);
        let mut fSlow53: f32 = f32::round(((self.fEntry110) as f32));
        let mut iSlow61: i32 = (((fSlow53 < 2.0f32)) as i32);
        let mut iSlow62: i32 = ((((fSlow53 == 0.0f32)) as i32) | (((fSlow53 == 3.0f32)) as i32));
        let mut fSlow54: f32 = f32::round(((self.fEntry112) as f32));
        let mut iSlow63: i32 = (((fSlow54 < 2.0f32)) as i32);
        let mut iSlow64: i32 = ((((fSlow54 == 0.0f32)) as i32) | (((fSlow54 == 3.0f32)) as i32));
        let mut iSlow65: i32 = ((f32::round(f32::round(((self.fHslider120) as f32)))) as i32);
        let mut iSlow66: i32 = ((f32::round(((self.fCheckbox121) as f32))) as i32);
        let mut iSlow67: i32 = ((f32::round(((self.fHslider123) as f32))) as i32);
        let mut iSlow68: i32 = ((f32::round((((iSlow67 & 31i32)) as f32))) as i32);
        let mut fSlow55: f32 = f32::round(((self.fHslider41) as f32));
        let mut iSlow69: i32 = (((fSlow55 >= 20.0f32)) as i32);
        let mut iSlow70: i32 = ((f32::round(fSlow55)) as i32);
        let mut fSlow56: f32 = (fSlow55 + 28.0f32);
        let mut fSlow57: f32 = (fSlow35 + 28.0f32);
        let mut fSlow58: f32 = f32::round(((self.fHslider48) as f32));
        let mut fSlow59: f32 = (329.0f32 * fSlow58);
        let mut fSlow60: f32 = f32::round(((self.fHslider50) as f32));
        let mut fSlow61: f32 = (329.0f32 * fSlow60);
        let mut iSlow71: i32 = ((f32::round((((((f32::max(0.0f32, f32::min(127.0f32, (127.0f32 * ((self.fHslider13) as f32))))) as i32)).wrapping_shr((1i32) as u32)) as f32))) as i32);
        let mut fSlow62: f32 = f32::round(((self.fHslider52) as f32));
        let mut fSlow63: f32 = f32::round(((self.fHslider56) as f32));
        let mut fSlow64: f32 = f32::powf(2.0f32, (0.0833333358168602f32 * (f32::round(((self.fHslider6) as f32)) + (17.312339782714844f32 * f32::ln((0.0022727272007614374f32 * ((self.fHslider5) as f32)))))));
        let mut fSlow65: f32 = f32::round(((17.312339782714844f32 * f32::ln(fSlow64)) + 69.0f32));
        let mut iSlow72: i32 = i32::min(31i32, i32::max(0i32, ((((0.3333333432674408f32 * fSlow65)) as i32)).wrapping_sub(7i32)));
        let mut iSlow73: i32 = (iSlow72 & 7i32);
        let mut iSlow74: i32 = (((iSlow73 == 3i32)) as i32);
        let mut iSlow75: i32 = (((iSlow73 > 0i32)) as i32);
        let mut iSlow76: i32 = (((iSlow73 < 4i32)) as i32);
        let mut fSlow66: f32 = ((iSlow72) as f32);
        let mut iSlow77: i32 = ((((fSlow63 * fSlow66)) as i32)).wrapping_shr((3i32) as u32);
        let mut iSlow78: i32 = (if ((((((fSlow63 == 3.0f32)) as i32) & iSlow74)) != 0) { (iSlow77).wrapping_sub(1i32) } else { (if (((((((fSlow63 == 7.0f32)) as i32) & iSlow75) & iSlow76)) != 0) { (iSlow77).wrapping_add(1i32) } else { iSlow77 }) });
        let mut fSlow67: f32 = ((iSlow78) as f32);
        let mut fSlow68: f32 = f32::min((fSlow62 + fSlow67), 99.0f32);
        let mut iSlow79: i32 = (((fSlow68 < 77.0f32)) as i32);
        let mut iSlow80: i32 = (((fSlow55 == 0.0f32)) as i32);
        let mut iSlow81: i32 = (iSlow79 & iSlow80);
        let mut fSlow69: f32 = (20.0f32 * (99.0f32 - fSlow68));
        let mut iSlow82: i32 = ((f32::round(fSlow68)) as i32);
        let mut fSlow70: f32 = f32::round(((self.fHslider44) as f32));
        let mut iSlow83: i32 = (((fSlow70 >= 20.0f32)) as i32);
        let mut iSlow84: i32 = ((f32::round(fSlow70)) as i32);
        let mut fSlow71: f32 = (fSlow70 + 28.0f32);
        let mut fSlow72: f32 = f32::round(((self.fHslider55) as f32));
        let mut fSlow73: f32 = f32::min((fSlow72 + fSlow67), 99.0f32);
        let mut iSlow85: i32 = (((fSlow73 < 77.0f32)) as i32);
        let mut fSlow74: f32 = (20.0f32 * (99.0f32 - fSlow73));
        let mut iSlow86: i32 = ((f32::round(fSlow73)) as i32);
        let mut iSlow87: i32 = i32::min((iSlow78).wrapping_add(((41i32).wrapping_mul(((fSlow62) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow88: i32 = (((self.fConst1 * (((((iSlow87 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow87).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut iSlow89: i32 = i32::min((iSlow78).wrapping_add(((41i32).wrapping_mul(((fSlow72) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow90: i32 = (((self.fConst1 * (((((iSlow89 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow89).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut fSlow75: f32 = f32::round(((self.fHslider23) as f32));
        let mut fSlow76: f32 = (self.fConst2 * (if ((0.010101010091602802f32 * fSlow75) <= 0.6565660238265991f32) { ((0.15806305408477783f32 * fSlow75) + 0.03647800162434578f32) } else { ((1.1002540588378906f32 * fSlow75) - 61.2059326171875f32) }));
        let mut fSlow77: f32 = (99.0f32 - f32::round(((self.fHslider24) as f32)));
        let mut iSlow91: i32 = ((((((fSlow77 == 99.0f32)) as i32) >= 1i32)) as i32);
        let mut iSlow92: i32 = ((fSlow77) as i32);
        let mut iSlow93: i32 = (((iSlow92 & 15i32)).wrapping_add(16i32)).wrapping_shl((((iSlow92).wrapping_shr((4i32) as u32)).wrapping_add(1i32)) as u32);
        let mut fSlow78: f32 = (if ((iSlow91) != 0) { 1.0f32 } else { (self.fConst3 * ((i32::max((iSlow93 & 65408i32), 128i32)) as f32)) });
        let mut fSlow79: f32 = (if ((iSlow91) != 0) { 1.0f32 } else { (self.fConst3 * ((iSlow93) as f32)) });
        let mut fSlow80: f32 = (2.6972606370634367e-9f32 * f32::round(((self.fHslider21) as f32)));
        let mut fSlow81: f32 = f32::round(((self.fHslider61) as f32));
        let mut fSlow82: f32 = (((if ((((fSlow81) as i32)) != 0) { ((f32::floor(((24204406.0f32 * f32::ln(((0.009999999776482582f32 * fSlow81) + 1.0f32))) + 0.5f32))) as i32) } else { 0i32 })) as f32);
        let mut fSlow83: f32 = f32::round(((self.fHslider59) as f32));
        let mut fSlow84: f32 = ((if (fSlow83 > 0.0f32) { (13457.0f32 * fSlow83) } else { 0.0f32 }) + ((((((4458616.0f32 * (fSlow81 + (((100i32).wrapping_mul((iSlow15 & 3i32))) as f32)))) as i32)).wrapping_shr((3i32) as u32)) as f32));
        let mut fSlow85: f32 = f32::round(((self.fHslider31) as f32));
        let mut iSlow94: i32 = ((f32::round(fSlow85)) as i32);
        let mut fSlow86: f32 = f32::round(((self.fHslider38) as f32));
        let mut iSlow95: i32 = ((f32::round(fSlow86)) as i32);
        let mut fSlow87: f32 = f32::round(((self.fHslider35) as f32));
        let mut iSlow96: i32 = ((f32::round(fSlow87)) as i32);
        let mut fSlow88: f32 = (7.891414134064689e-5f32 * f32::round(((self.fHslider39) as f32)));
        let mut fSlow89: f32 = f32::round(((self.fHslider0) as f32));
        let mut iSlow97: i32 = (((fSlow89 >= 20.0f32)) as i32);
        let mut iSlow98: i32 = ((f32::round(fSlow89)) as i32);
        let mut fSlow90: f32 = (fSlow89 + 28.0f32);
        let mut fSlow91: f32 = (fSlow40 + 28.0f32);
        let mut fSlow92: f32 = f32::round(((self.fHslider9) as f32));
        let mut fSlow93: f32 = (329.0f32 * fSlow92);
        let mut fSlow94: f32 = f32::round(((self.fHslider11) as f32));
        let mut fSlow95: f32 = (329.0f32 * fSlow94);
        let mut fSlow96: f32 = f32::round(((self.fHslider14) as f32));
        let mut fSlow97: f32 = f32::round(((self.fHslider18) as f32));
        let mut iSlow99: i32 = ((((fSlow97 * fSlow66)) as i32)).wrapping_shr((3i32) as u32);
        let mut iSlow100: i32 = (if ((((((fSlow97 == 3.0f32)) as i32) & iSlow74)) != 0) { (iSlow99).wrapping_sub(1i32) } else { (if (((((((fSlow97 == 7.0f32)) as i32) & iSlow75) & iSlow76)) != 0) { (iSlow99).wrapping_add(1i32) } else { iSlow99 }) });
        let mut fSlow98: f32 = ((iSlow100) as f32);
        let mut fSlow99: f32 = f32::min((fSlow96 + fSlow98), 99.0f32);
        let mut iSlow101: i32 = (((fSlow99 < 77.0f32)) as i32);
        let mut iSlow102: i32 = (((fSlow89 == 0.0f32)) as i32);
        let mut iSlow103: i32 = (iSlow101 & iSlow102);
        let mut fSlow100: f32 = (20.0f32 * (99.0f32 - fSlow99));
        let mut iSlow104: i32 = ((f32::round(fSlow99)) as i32);
        let mut fSlow101: f32 = f32::round(((self.fHslider3) as f32));
        let mut iSlow105: i32 = (((fSlow101 >= 20.0f32)) as i32);
        let mut iSlow106: i32 = ((f32::round(fSlow101)) as i32);
        let mut fSlow102: f32 = (fSlow101 + 28.0f32);
        let mut fSlow103: f32 = f32::round(((self.fHslider17) as f32));
        let mut fSlow104: f32 = f32::min((fSlow103 + fSlow98), 99.0f32);
        let mut iSlow107: i32 = (((fSlow104 < 77.0f32)) as i32);
        let mut fSlow105: f32 = (20.0f32 * (99.0f32 - fSlow104));
        let mut iSlow108: i32 = ((f32::round(fSlow104)) as i32);
        let mut iSlow109: i32 = i32::min((iSlow100).wrapping_add(((41i32).wrapping_mul(((fSlow96) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow110: i32 = (((self.fConst1 * (((((iSlow109 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow109).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut iSlow111: i32 = i32::min((iSlow100).wrapping_add(((41i32).wrapping_mul(((fSlow103) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow112: i32 = (((self.fConst1 * (((((iSlow111 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow111).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut fSlow106: f32 = f32::round(((self.fHslider30) as f32));
        let mut fSlow107: f32 = (((if ((((fSlow106) as i32)) != 0) { ((f32::floor(((24204406.0f32 * f32::ln(((0.009999999776482582f32 * fSlow106) + 1.0f32))) + 0.5f32))) as i32) } else { 0i32 })) as f32);
        let mut fSlow108: f32 = f32::round(((self.fHslider28) as f32));
        let mut fSlow109: f32 = ((if (fSlow108 > 0.0f32) { (13457.0f32 * fSlow108) } else { 0.0f32 }) + ((((((4458616.0f32 * (fSlow106 + (((100i32).wrapping_mul((iSlow27 & 3i32))) as f32)))) as i32)).wrapping_shr((3i32) as u32)) as f32));
        let mut fSlow110: f32 = f32::round(((self.fHslider83) as f32));
        let mut iSlow113: i32 = (((fSlow110 >= 20.0f32)) as i32);
        let mut iSlow114: i32 = ((f32::round(fSlow110)) as i32);
        let mut fSlow111: f32 = (fSlow110 + 28.0f32);
        let mut fSlow112: f32 = (fSlow43 + 28.0f32);
        let mut fSlow113: f32 = f32::round(((self.fHslider90) as f32));
        let mut fSlow114: f32 = (329.0f32 * fSlow113);
        let mut fSlow115: f32 = f32::round(((self.fHslider92) as f32));
        let mut fSlow116: f32 = (329.0f32 * fSlow115);
        let mut fSlow117: f32 = f32::round(((self.fHslider94) as f32));
        let mut fSlow118: f32 = f32::round(((self.fHslider98) as f32));
        let mut iSlow115: i32 = ((((fSlow118 * fSlow66)) as i32)).wrapping_shr((3i32) as u32);
        let mut iSlow116: i32 = (if ((((((fSlow118 == 3.0f32)) as i32) & iSlow74)) != 0) { (iSlow115).wrapping_sub(1i32) } else { (if (((((((fSlow118 == 7.0f32)) as i32) & iSlow75) & iSlow76)) != 0) { (iSlow115).wrapping_add(1i32) } else { iSlow115 }) });
        let mut fSlow119: f32 = ((iSlow116) as f32);
        let mut fSlow120: f32 = f32::min((fSlow117 + fSlow119), 99.0f32);
        let mut iSlow117: i32 = (((fSlow120 < 77.0f32)) as i32);
        let mut iSlow118: i32 = (((fSlow110 == 0.0f32)) as i32);
        let mut iSlow119: i32 = (iSlow117 & iSlow118);
        let mut fSlow121: f32 = (20.0f32 * (99.0f32 - fSlow120));
        let mut iSlow120: i32 = ((f32::round(fSlow120)) as i32);
        let mut fSlow122: f32 = f32::round(((self.fHslider86) as f32));
        let mut iSlow121: i32 = (((fSlow122 >= 20.0f32)) as i32);
        let mut iSlow122: i32 = ((f32::round(fSlow122)) as i32);
        let mut fSlow123: f32 = (fSlow122 + 28.0f32);
        let mut fSlow124: f32 = f32::round(((self.fHslider97) as f32));
        let mut fSlow125: f32 = f32::min((fSlow124 + fSlow119), 99.0f32);
        let mut iSlow123: i32 = (((fSlow125 < 77.0f32)) as i32);
        let mut fSlow126: f32 = (20.0f32 * (99.0f32 - fSlow125));
        let mut iSlow124: i32 = ((f32::round(fSlow125)) as i32);
        let mut iSlow125: i32 = i32::min((iSlow116).wrapping_add(((41i32).wrapping_mul(((fSlow117) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow126: i32 = (((self.fConst1 * (((((iSlow125 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow125).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut iSlow127: i32 = i32::min((iSlow116).wrapping_add(((41i32).wrapping_mul(((fSlow124) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow128: i32 = (((self.fConst1 * (((((iSlow127 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow127).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut fSlow127: f32 = f32::round(((self.fHslider103) as f32));
        let mut fSlow128: f32 = (((if ((((fSlow127) as i32)) != 0) { ((f32::floor(((24204406.0f32 * f32::ln(((0.009999999776482582f32 * fSlow127) + 1.0f32))) + 0.5f32))) as i32) } else { 0i32 })) as f32);
        let mut fSlow129: f32 = f32::round(((self.fHslider101) as f32));
        let mut fSlow130: f32 = ((if (fSlow129 > 0.0f32) { (13457.0f32 * fSlow129) } else { 0.0f32 }) + ((((((4458616.0f32 * (fSlow127 + (((100i32).wrapping_mul((iSlow37 & 3i32))) as f32)))) as i32)).wrapping_shr((3i32) as u32)) as f32));
        let mut fSlow131: f32 = f32::round(((self.fHslider62) as f32));
        let mut iSlow129: i32 = (((fSlow131 >= 20.0f32)) as i32);
        let mut iSlow130: i32 = ((f32::round(fSlow131)) as i32);
        let mut fSlow132: f32 = (fSlow131 + 28.0f32);
        let mut fSlow133: f32 = (fSlow46 + 28.0f32);
        let mut fSlow134: f32 = f32::round(((self.fHslider69) as f32));
        let mut fSlow135: f32 = (329.0f32 * fSlow134);
        let mut fSlow136: f32 = f32::round(((self.fHslider71) as f32));
        let mut fSlow137: f32 = (329.0f32 * fSlow136);
        let mut fSlow138: f32 = f32::round(((self.fHslider73) as f32));
        let mut fSlow139: f32 = f32::round(((self.fHslider77) as f32));
        let mut iSlow131: i32 = ((((fSlow139 * fSlow66)) as i32)).wrapping_shr((3i32) as u32);
        let mut iSlow132: i32 = (if ((((((fSlow139 == 3.0f32)) as i32) & iSlow74)) != 0) { (iSlow131).wrapping_sub(1i32) } else { (if (((((((fSlow139 == 7.0f32)) as i32) & iSlow75) & iSlow76)) != 0) { (iSlow131).wrapping_add(1i32) } else { iSlow131 }) });
        let mut fSlow140: f32 = ((iSlow132) as f32);
        let mut fSlow141: f32 = f32::min((fSlow138 + fSlow140), 99.0f32);
        let mut iSlow133: i32 = (((fSlow141 < 77.0f32)) as i32);
        let mut iSlow134: i32 = (((fSlow131 == 0.0f32)) as i32);
        let mut iSlow135: i32 = (iSlow133 & iSlow134);
        let mut fSlow142: f32 = (20.0f32 * (99.0f32 - fSlow141));
        let mut iSlow136: i32 = ((f32::round(fSlow141)) as i32);
        let mut fSlow143: f32 = f32::round(((self.fHslider65) as f32));
        let mut iSlow137: i32 = (((fSlow143 >= 20.0f32)) as i32);
        let mut iSlow138: i32 = ((f32::round(fSlow143)) as i32);
        let mut fSlow144: f32 = (fSlow143 + 28.0f32);
        let mut fSlow145: f32 = f32::round(((self.fHslider76) as f32));
        let mut fSlow146: f32 = f32::min((fSlow145 + fSlow140), 99.0f32);
        let mut iSlow139: i32 = (((fSlow146 < 77.0f32)) as i32);
        let mut fSlow147: f32 = (20.0f32 * (99.0f32 - fSlow146));
        let mut iSlow140: i32 = ((f32::round(fSlow146)) as i32);
        let mut iSlow141: i32 = i32::min((iSlow132).wrapping_add(((41i32).wrapping_mul(((fSlow138) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow142: i32 = (((self.fConst1 * (((((iSlow141 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow141).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut iSlow143: i32 = i32::min((iSlow132).wrapping_add(((41i32).wrapping_mul(((fSlow145) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow144: i32 = (((self.fConst1 * (((((iSlow143 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow143).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut fSlow148: f32 = f32::round(((self.fHslider82) as f32));
        let mut fSlow149: f32 = (((if ((((fSlow148) as i32)) != 0) { ((f32::floor(((24204406.0f32 * f32::ln(((0.009999999776482582f32 * fSlow148) + 1.0f32))) + 0.5f32))) as i32) } else { 0i32 })) as f32);
        let mut fSlow150: f32 = f32::round(((self.fHslider80) as f32));
        let mut fSlow151: f32 = ((if (fSlow150 > 0.0f32) { (13457.0f32 * fSlow150) } else { 0.0f32 }) + ((((((4458616.0f32 * (fSlow148 + (((100i32).wrapping_mul((iSlow47 & 3i32))) as f32)))) as i32)).wrapping_shr((3i32) as u32)) as f32));
        let mut fSlow152: f32 = f32::round(((self.fHslider126) as f32));
        let mut iSlow145: i32 = (((fSlow152 >= 20.0f32)) as i32);
        let mut iSlow146: i32 = ((f32::round(fSlow152)) as i32);
        let mut fSlow153: f32 = (fSlow152 + 28.0f32);
        let mut fSlow154: f32 = (fSlow49 + 28.0f32);
        let mut fSlow155: f32 = f32::round(((self.fHslider133) as f32));
        let mut fSlow156: f32 = (329.0f32 * fSlow155);
        let mut fSlow157: f32 = f32::round(((self.fHslider135) as f32));
        let mut fSlow158: f32 = (329.0f32 * fSlow157);
        let mut fSlow159: f32 = f32::round(((self.fHslider137) as f32));
        let mut fSlow160: f32 = f32::round(((self.fHslider141) as f32));
        let mut iSlow147: i32 = ((((fSlow160 * fSlow66)) as i32)).wrapping_shr((3i32) as u32);
        let mut iSlow148: i32 = (if ((((((fSlow160 == 3.0f32)) as i32) & iSlow74)) != 0) { (iSlow147).wrapping_sub(1i32) } else { (if (((((((fSlow160 == 7.0f32)) as i32) & iSlow75) & iSlow76)) != 0) { (iSlow147).wrapping_add(1i32) } else { iSlow147 }) });
        let mut fSlow161: f32 = ((iSlow148) as f32);
        let mut fSlow162: f32 = f32::min((fSlow159 + fSlow161), 99.0f32);
        let mut iSlow149: i32 = (((fSlow162 < 77.0f32)) as i32);
        let mut iSlow150: i32 = (((fSlow152 == 0.0f32)) as i32);
        let mut iSlow151: i32 = (iSlow149 & iSlow150);
        let mut fSlow163: f32 = (20.0f32 * (99.0f32 - fSlow162));
        let mut iSlow152: i32 = ((f32::round(fSlow162)) as i32);
        let mut fSlow164: f32 = f32::round(((self.fHslider129) as f32));
        let mut iSlow153: i32 = (((fSlow164 >= 20.0f32)) as i32);
        let mut iSlow154: i32 = ((f32::round(fSlow164)) as i32);
        let mut fSlow165: f32 = (fSlow164 + 28.0f32);
        let mut fSlow166: f32 = f32::round(((self.fHslider140) as f32));
        let mut fSlow167: f32 = f32::min((fSlow166 + fSlow161), 99.0f32);
        let mut iSlow155: i32 = (((fSlow167 < 77.0f32)) as i32);
        let mut fSlow168: f32 = (20.0f32 * (99.0f32 - fSlow167));
        let mut iSlow156: i32 = ((f32::round(fSlow167)) as i32);
        let mut iSlow157: i32 = i32::min((iSlow148).wrapping_add(((41i32).wrapping_mul(((fSlow159) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow158: i32 = (((self.fConst1 * (((((iSlow157 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow157).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut iSlow159: i32 = i32::min((iSlow148).wrapping_add(((41i32).wrapping_mul(((fSlow166) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow160: i32 = (((self.fConst1 * (((((iSlow159 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow159).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut fSlow169: f32 = f32::round(((self.fHslider146) as f32));
        let mut fSlow170: f32 = (((if ((((fSlow169) as i32)) != 0) { ((f32::floor(((24204406.0f32 * f32::ln(((0.009999999776482582f32 * fSlow169) + 1.0f32))) + 0.5f32))) as i32) } else { 0i32 })) as f32);
        let mut fSlow171: f32 = f32::round(((self.fHslider144) as f32));
        let mut fSlow172: f32 = ((if (fSlow171 > 0.0f32) { (13457.0f32 * fSlow171) } else { 0.0f32 }) + ((((((4458616.0f32 * (fSlow169 + (((100i32).wrapping_mul((iSlow57 & 3i32))) as f32)))) as i32)).wrapping_shr((3i32) as u32)) as f32));
        let mut fSlow173: f32 = f32::round(((self.fHslider104) as f32));
        let mut iSlow161: i32 = (((fSlow173 >= 20.0f32)) as i32);
        let mut iSlow162: i32 = ((f32::round(fSlow173)) as i32);
        let mut fSlow174: f32 = (fSlow173 + 28.0f32);
        let mut fSlow175: f32 = (fSlow52 + 28.0f32);
        let mut fSlow176: f32 = f32::round(((self.fHslider111) as f32));
        let mut fSlow177: f32 = (329.0f32 * fSlow176);
        let mut fSlow178: f32 = f32::round(((self.fHslider113) as f32));
        let mut fSlow179: f32 = (329.0f32 * fSlow178);
        let mut fSlow180: f32 = f32::round(((self.fHslider115) as f32));
        let mut fSlow181: f32 = f32::round(((self.fHslider119) as f32));
        let mut iSlow163: i32 = ((((fSlow181 * fSlow66)) as i32)).wrapping_shr((3i32) as u32);
        let mut iSlow164: i32 = (if ((((((fSlow181 == 3.0f32)) as i32) & iSlow74)) != 0) { (iSlow163).wrapping_sub(1i32) } else { (if (((((((fSlow181 == 7.0f32)) as i32) & iSlow75) & iSlow76)) != 0) { (iSlow163).wrapping_add(1i32) } else { iSlow163 }) });
        let mut fSlow182: f32 = ((iSlow164) as f32);
        let mut fSlow183: f32 = f32::min((fSlow180 + fSlow182), 99.0f32);
        let mut iSlow165: i32 = (((fSlow183 < 77.0f32)) as i32);
        let mut iSlow166: i32 = (((fSlow173 == 0.0f32)) as i32);
        let mut iSlow167: i32 = (iSlow165 & iSlow166);
        let mut fSlow184: f32 = (20.0f32 * (99.0f32 - fSlow183));
        let mut iSlow168: i32 = ((f32::round(fSlow183)) as i32);
        let mut fSlow185: f32 = f32::round(((self.fHslider107) as f32));
        let mut iSlow169: i32 = (((fSlow185 >= 20.0f32)) as i32);
        let mut iSlow170: i32 = ((f32::round(fSlow185)) as i32);
        let mut fSlow186: f32 = (fSlow185 + 28.0f32);
        let mut fSlow187: f32 = f32::round(((self.fHslider118) as f32));
        let mut fSlow188: f32 = f32::min((fSlow187 + fSlow182), 99.0f32);
        let mut iSlow171: i32 = (((fSlow188 < 77.0f32)) as i32);
        let mut fSlow189: f32 = (20.0f32 * (99.0f32 - fSlow188));
        let mut iSlow172: i32 = ((f32::round(fSlow188)) as i32);
        let mut iSlow173: i32 = i32::min((iSlow164).wrapping_add(((41i32).wrapping_mul(((fSlow180) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow174: i32 = (((self.fConst1 * (((((iSlow173 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow173).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut iSlow175: i32 = i32::min((iSlow164).wrapping_add(((41i32).wrapping_mul(((fSlow187) as i32))).wrapping_shr((6i32) as u32)), 63i32);
        let mut iSlow176: i32 = (((self.fConst1 * (((((iSlow175 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iSlow175).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32);
        let mut fSlow190: f32 = f32::round(((self.fHslider124) as f32));
        let mut fSlow191: f32 = (((if ((((fSlow190) as i32)) != 0) { ((f32::floor(((24204406.0f32 * f32::ln(((0.009999999776482582f32 * fSlow190) + 1.0f32))) + 0.5f32))) as i32) } else { 0i32 })) as f32);
        let mut fSlow192: f32 = f32::round(((self.fHslider122) as f32));
        let mut fSlow193: f32 = ((if (fSlow192 > 0.0f32) { (13457.0f32 * fSlow192) } else { 0.0f32 }) + ((((((4458616.0f32 * (fSlow190 + (((100i32).wrapping_mul((iSlow67 & 3i32))) as f32)))) as i32)).wrapping_shr((3i32) as u32)) as f32));
        let mut fSlow194: f32 = f32::round(((self.fHslider125) as f32));
        let mut fSlow195: f32 = (if (fSlow194 == 0.0f32) { 0.0f32 } else { f32::powf(2.0f32, (fSlow194 - 7.0f32)) });
        let mut fSlow196: f32 = f32::round(((self.fHslider46) as f32));
        let mut iSlow177: i32 = ((((fSlow65 - (fSlow196 + 17.0f32)) >= 0.0f32)) as i32);
        let mut fSlow197: f32 = (18.0f32 - fSlow65);
        let mut fSlow198: f32 = (fSlow196 + fSlow197);
        let mut iSlow178: i32 = ((f32::round((0.3333333432674408f32 * fSlow198))) as i32);
        let mut iSlow179: i32 = (((((109.66666412353516f32 * fSlow58) * fSlow198)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow199: f32 = (fSlow65 - (fSlow196 + 16.0f32));
        let mut iSlow180: i32 = ((f32::round((0.3333333432674408f32 * fSlow199))) as i32);
        let mut iSlow181: i32 = (((((109.66666412353516f32 * fSlow60) * fSlow199)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow200: f32 = f32::round(((self.fHslider7) as f32));
        let mut iSlow182: i32 = ((((fSlow65 - (fSlow200 + 17.0f32)) >= 0.0f32)) as i32);
        let mut fSlow201: f32 = (fSlow200 + fSlow197);
        let mut iSlow183: i32 = ((f32::round((0.3333333432674408f32 * fSlow201))) as i32);
        let mut iSlow184: i32 = (((((109.66666412353516f32 * fSlow92) * fSlow201)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow202: f32 = (fSlow65 - (fSlow200 + 16.0f32));
        let mut iSlow185: i32 = ((f32::round((0.3333333432674408f32 * fSlow202))) as i32);
        let mut iSlow186: i32 = (((((109.66666412353516f32 * fSlow94) * fSlow202)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow203: f32 = f32::round(((self.fHslider88) as f32));
        let mut iSlow187: i32 = ((((fSlow65 - (fSlow203 + 17.0f32)) >= 0.0f32)) as i32);
        let mut fSlow204: f32 = (fSlow203 + fSlow197);
        let mut iSlow188: i32 = ((f32::round((0.3333333432674408f32 * fSlow204))) as i32);
        let mut iSlow189: i32 = (((((109.66666412353516f32 * fSlow113) * fSlow204)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow205: f32 = (fSlow65 - (fSlow203 + 16.0f32));
        let mut iSlow190: i32 = ((f32::round((0.3333333432674408f32 * fSlow205))) as i32);
        let mut iSlow191: i32 = (((((109.66666412353516f32 * fSlow115) * fSlow205)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow206: f32 = f32::round(((self.fHslider67) as f32));
        let mut iSlow192: i32 = ((((fSlow65 - (fSlow206 + 17.0f32)) >= 0.0f32)) as i32);
        let mut fSlow207: f32 = (fSlow206 + fSlow197);
        let mut iSlow193: i32 = ((f32::round((0.3333333432674408f32 * fSlow207))) as i32);
        let mut iSlow194: i32 = (((((109.66666412353516f32 * fSlow134) * fSlow207)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow208: f32 = (fSlow65 - (fSlow206 + 16.0f32));
        let mut iSlow195: i32 = ((f32::round((0.3333333432674408f32 * fSlow208))) as i32);
        let mut iSlow196: i32 = (((((109.66666412353516f32 * fSlow136) * fSlow208)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow209: f32 = f32::round(((self.fHslider131) as f32));
        let mut iSlow197: i32 = ((((fSlow65 - (fSlow209 + 17.0f32)) >= 0.0f32)) as i32);
        let mut fSlow210: f32 = (fSlow209 + fSlow197);
        let mut iSlow198: i32 = ((f32::round((0.3333333432674408f32 * fSlow210))) as i32);
        let mut iSlow199: i32 = (((((109.66666412353516f32 * fSlow155) * fSlow210)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow211: f32 = (fSlow65 - (fSlow209 + 16.0f32));
        let mut iSlow200: i32 = ((f32::round((0.3333333432674408f32 * fSlow211))) as i32);
        let mut iSlow201: i32 = (((((109.66666412353516f32 * fSlow157) * fSlow211)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow212: f32 = f32::round(((self.fHslider109) as f32));
        let mut iSlow202: i32 = ((((fSlow65 - (fSlow212 + 17.0f32)) >= 0.0f32)) as i32);
        let mut fSlow213: f32 = (fSlow212 + fSlow197);
        let mut iSlow203: i32 = ((f32::round((0.3333333432674408f32 * fSlow213))) as i32);
        let mut iSlow204: i32 = (((((109.66666412353516f32 * fSlow176) * fSlow213)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow214: f32 = (fSlow65 - (fSlow212 + 16.0f32));
        let mut iSlow205: i32 = ((f32::round((0.3333333432674408f32 * fSlow214))) as i32);
        let mut iSlow206: i32 = (((((109.66666412353516f32 * fSlow178) * fSlow214)) as i32)).wrapping_shr((12i32) as u32);
        let mut fSlow215: f32 = f32::ln((440.0f32 * fSlow64));
        let mut fSlow216: f32 = f32::exp((-1.0f32 * (0.5713072419166565f32 * fSlow215)));
        let mut fSlow217: f32 = (fSlow215 * (((72267.4453125f32 * fSlow83) * fSlow216) + 24204406.0f32));
        let mut fSlow218: f32 = (fSlow215 * (((72267.4453125f32 * fSlow108) * fSlow216) + 24204406.0f32));
        let mut fSlow219: f32 = (fSlow215 * (((72267.4453125f32 * fSlow129) * fSlow216) + 24204406.0f32));
        let mut fSlow220: f32 = (fSlow215 * (((72267.4453125f32 * fSlow150) * fSlow216) + 24204406.0f32));
        let mut fSlow221: f32 = (fSlow215 * (((72267.4453125f32 * fSlow171) * fSlow216) + 24204406.0f32));
        let mut fSlow222: f32 = (fSlow215 * (((72267.4453125f32 * fSlow192) * fSlow216) + 24204406.0f32));
        let mut outputs_iter = outputs.iter_mut();
        let output0 = outputs_iter.nth(0).expect("missing output channel").as_mut();
        let output1 = outputs_iter.nth(0).expect("missing output channel").as_mut();
        for i0 in 0..count {
            self.fVec12[(0i32) as usize] = fSlow0;
            let mut fTemp0: f32 = self.fVec12[(1i32) as usize];
            let mut iTemp0: i32 = ((((((fSlow0 < fTemp0)) as i32) >= 1i32)) as i32);
            let mut iTemp1: i32 = (((fSlow0 > fTemp0)) as i32);
            let mut iTemp2: i32 = (((iTemp1 >= 1i32)) as i32);
            let mut fTemp1: f32 = (((iTbl242[(i32::min(iSlow71, 63i32)) as usize]).wrapping_sub(239i32)) as f32);
            let mut iTemp3: i32 = (if ((iSlow3) != 0) { iSlow179 } else { ((((fSlow59 * ((iTbl129[(i32::max(i32::min(32i32, iSlow178), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut iTemp4: i32 = (if ((iSlow5) != 0) { iSlow181 } else { ((((fSlow61 * ((iTbl129[(i32::max(i32::min(32i32, iSlow180), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut fTemp2: f32 = f32::max((((((((((fSlow3 * fTemp1) + 7.0f32)) as i32)).wrapping_shr((3i32) as u32)).wrapping_shl((4i32) as u32)) as f32) + (32.0f32 * f32::min(((if ((iSlow0) != 0) { fSlow57 } else { ((iTbl59[(i32::min(iSlow1, 19i32)) as usize]) as f32) }) + (((if ((iSlow177) != 0) { (if ((iSlow4) != 0) { (-1i32).wrapping_mul(iTemp4) } else { iTemp4 }) } else { (if ((iSlow2) != 0) { (-1i32).wrapping_mul(iTemp3) } else { iTemp3 }) })) as f32)), 127.0f32))), 0.0f32);
            let mut iTemp5: i32 = (((f32::max((((((((((if ((iSlow69) != 0) { fSlow56 } else { ((iTbl59[(i32::min(iSlow70, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp2) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut fTemp3: f32 = (if ((iSlow79) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow82), 0i32)) as usize]) as f32) } else { fSlow69 });
            let mut iTemp6: i32 = (((f32::max((((((((((if ((iSlow83) != 0) { fSlow71 } else { ((iTbl59[(i32::min(iSlow84, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp2) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp7: i32 = (if ((iTemp2) != 0) { 0i32 } else { self.iRec15971[(1i32) as usize] });
            let mut iTemp8: i32 = (if ((iTemp0) != 0) { (if (iTemp6 == iTemp7) { (((self.fConst1 * (if ((iSlow85) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow86), 0i32)) as usize]) as f32) } else { fSlow74 }))) as i32) } else { 0i32 }) } else { (if ((iTemp2) != 0) { (if ((((((iTemp5 == 0i32)) as i32) | iSlow80)) != 0) { (((self.fConst1 * (if ((iSlow81) != 0) { (0.05000000074505806f32 * fTemp3) } else { fTemp3 }))) as i32) } else { 0i32 }) } else { self.iRec15971_6[(1i32) as usize] }) });
            let mut iTemp9: i32 = (((iTemp8 != 0i32)) as i32);
            let mut iTemp10: i32 = ((((iTemp9 & (((iTemp8 <= 1i32)) as i32)) >= 1i32)) as i32);
            let mut iTemp11: i32 = (if ((iTemp0) != 0) { 3i32 } else { (if ((iTemp2) != 0) { 0i32 } else { self.iRec15971_1[(1i32) as usize] }) });
            let mut iTemp12: i32 = (iTemp11).wrapping_add(1i32);
            let mut iTemp13: i32 = (if ((iTemp10) != 0) { iTemp12 } else { iTemp11 });
            let mut iTemp14: i32 = (if ((iTemp0) != 0) { 0i32 } else { (if ((iTemp2) != 0) { 1i32 } else { self.iRec15971_5[(1i32) as usize] }) });
            let mut iTemp15: i32 = (((((((iTemp13 < 3i32)) as i32) | ((((iTemp13 < 4i32)) as i32) & (iTemp14 ^ -1i32))) >= 1i32)) as i32);
            let mut iTemp16: i32 = ((((((iTemp12 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp17: i32 = (((iTemp12 >= 2i32)) as i32);
            let mut iTemp18: i32 = (((iTemp12 >= 1i32)) as i32);
            let mut iTemp19: i32 = (((iTemp12 >= 3i32)) as i32);
            let mut fTemp4: f32 = (if ((iTemp17) != 0) { (if ((iTemp19) != 0) { fSlow70 } else { fSlow2 }) } else { (if ((iTemp18) != 0) { fSlow1 } else { fSlow55 }) });
            let mut iTemp20: i32 = (((f32::max((((((((((if (fTemp4 >= 20.0f32) { (fTemp4 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp4)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp2) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp21: i32 = (((iTemp12 == 0i32)) as i32);
            let mut iTemp22: i32 = (((fTemp4 == 0.0f32)) as i32);
            let mut fTemp5: f32 = (if ((iTemp17) != 0) { (if ((iTemp19) != 0) { fSlow72 } else { fSlow5 }) } else { (if ((iTemp18) != 0) { fSlow4 } else { fSlow62 }) });
            let mut fTemp6: f32 = f32::min((fSlow67 + fTemp5), 99.0f32);
            let mut iTemp23: i32 = (((fTemp6 < 77.0f32)) as i32);
            let mut fTemp7: f32 = (if ((iTemp23) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp6)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp6)) });
            let mut iTemp24: i32 = (if ((iTemp10) != 0) { (if ((iTemp16) != 0) { (if ((((((iTemp20 == iTemp7)) as i32) | (iTemp21 & iTemp22))) != 0) { (((self.fConst1 * (if ((((iTemp23 & iTemp21) & iTemp22)) != 0) { (0.05000000074505806f32 * fTemp7) } else { fTemp7 }))) as i32) } else { 0i32 }) } else { 0i32 }) } else { (iTemp8).wrapping_sub((if ((iTemp9) != 0) { 1i32 } else { 0i32 })) });
            let mut iTemp25: i32 = (if ((iTemp0) != 0) { (((iTemp6 > iTemp7)) as i32) } else { (if ((iTemp2) != 0) { (((iTemp5 > 0i32)) as i32) } else { self.iRec15971_3[(1i32) as usize] }) });
            let mut iTemp26: i32 = (if ((iTemp10) != 0) { (if ((iTemp16) != 0) { (((iTemp20 > iTemp7)) as i32) } else { iTemp25 }) } else { iTemp25 });
            let mut iTemp27: i32 = ((((iTemp24 == 0i32)) as i32)).wrapping_mul(((((iTemp26 == 0i32)) as i32)).wrapping_add(1i32));
            let mut iTemp28: i32 = (((iTemp27 >= 2i32)) as i32);
            let mut iTemp29: i32 = (((iTemp27 >= 1i32)) as i32);
            let mut iTemp30: i32 = i32::max(112459776i32, iTemp7);
            let mut iTemp31: i32 = (if ((iTemp0) != 0) { iSlow90 } else { (if ((iTemp2) != 0) { iSlow88 } else { self.iRec15971_4[(1i32) as usize] }) });
            let mut iTemp32: i32 = i32::min((iSlow78).wrapping_add(((41i32).wrapping_mul(((fTemp5) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp33: i32 = (if ((iTemp10) != 0) { (if ((iTemp16) != 0) { (((self.fConst1 * (((((iTemp32 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp32).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp31 }) } else { iTemp31 });
            let mut iTemp34: i32 = (iTemp30).wrapping_add((((285212672i32).wrapping_sub(iTemp30)).wrapping_shr((24i32) as u32)).wrapping_mul(iTemp33));
            let mut iTemp35: i32 = (if ((iTemp0) != 0) { iTemp6 } else { (if ((iTemp2) != 0) { iTemp5 } else { self.iRec15971_2[(1i32) as usize] }) });
            let mut iTemp36: i32 = (if ((iTemp10) != 0) { (if ((iTemp16) != 0) { iTemp20 } else { iTemp35 }) } else { iTemp35 });
            let mut iTemp37: i32 = ((((((iTemp34 >= iTemp36)) as i32) >= 1i32)) as i32);
            let mut iTemp38: i32 = (iTemp13).wrapping_add(1i32);
            let mut iTemp39: i32 = (iTemp7).wrapping_sub(iTemp33);
            let mut iTemp40: i32 = ((((((iTemp39 <= iTemp36)) as i32) >= 1i32)) as i32);
            let mut iTemp41: i32 = ((((((iTemp38 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp42: i32 = (((iTemp38 >= 2i32)) as i32);
            let mut iTemp43: i32 = (((iTemp38 >= 1i32)) as i32);
            let mut iTemp44: i32 = (((iTemp38 >= 3i32)) as i32);
            let mut fTemp8: f32 = (if ((iTemp42) != 0) { (if ((iTemp44) != 0) { fSlow70 } else { fSlow2 }) } else { (if ((iTemp43) != 0) { fSlow1 } else { fSlow55 }) });
            let mut iTemp45: i32 = (((f32::max(((fTemp2 + (((((((if (fTemp8 >= 20.0f32) { (fTemp8 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp8)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32)) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp46: i32 = (if ((iTemp41) != 0) { iTemp45 } else { iTemp36 });
            let mut iTemp47: i32 = (if ((iTemp41) != 0) { (((iTemp45 > iTemp36)) as i32) } else { iTemp26 });
            let mut fTemp9: f32 = (if ((iTemp42) != 0) { (if ((iTemp44) != 0) { fSlow72 } else { fSlow5 }) } else { (if ((iTemp43) != 0) { fSlow4 } else { fSlow62 }) });
            let mut iTemp48: i32 = i32::min((iSlow78).wrapping_add(((41i32).wrapping_mul(((fTemp9) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp49: i32 = (if ((iTemp41) != 0) { (((self.fConst1 * (((((iTemp48 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp48).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp33 });
            let mut iTemp50: i32 = (((iTemp38 == 0i32)) as i32);
            let mut iTemp51: i32 = (((fTemp8 == 0.0f32)) as i32);
            let mut fTemp10: f32 = f32::min((fSlow67 + fTemp9), 99.0f32);
            let mut iTemp52: i32 = (((fTemp10 < 77.0f32)) as i32);
            let mut fTemp11: f32 = (if ((iTemp52) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp10)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp10)) });
            let mut iTemp53: i32 = (if ((iTemp41) != 0) { (if ((((((iTemp45 == iTemp36)) as i32) | (iTemp50 & iTemp51))) != 0) { (((self.fConst1 * (if ((((iTemp52 & iTemp50) & iTemp51)) != 0) { (0.05000000074505806f32 * fTemp11) } else { fTemp11 }))) as i32) } else { 0i32 }) } else { iTemp24 });
            let mut fTemp12: f32 = (if ((iTemp1) != 0) { 0.0f32 } else { self.fRec16024_1[(1i32) as usize] });
            let mut fTemp13: f32 = (fTemp12 + (if (fTemp12 < 1.0f32) { fSlow79 } else { fSlow78 }));
            let mut iTemp54: i32 = ((((fTemp13 <= 2.0f32)) as i32)).wrapping_mul((2i32).wrapping_sub((((fTemp13 < 1.0f32)) as i32)));
            let mut iTemp55: i32 = (((iTemp54 >= 2i32)) as i32);
            let mut iTemp56: i32 = (((iTemp54 >= 1i32)) as i32);
            let mut iTemp57: i32 = self.iRec16024_4[(1i32) as usize];
            let mut fTemp14: f32 = (if (((iSlow7 & iTemp1)) != 0) { 0.0f32 } else { self.fRec16024[(1i32) as usize] });
            let mut fTemp15: f32 = f32::floor((fSlow76 + fTemp14));
            let mut fTemp16: f32 = (fSlow76 + (fTemp14 - fTemp15));
            let mut iTemp58: i32 = (if (fTemp16 < fSlow76) { (((179i32).wrapping_mul(iTemp57)).wrapping_add(17i32) & 255i32) } else { iTemp57 });
            let mut iTemp59: i32 = (if ((iTemp2) != 0) { 0i32 } else { (if ((iTemp0) != 0) { 3i32 } else { self.iRec16090_1[(1i32) as usize] }) });
            let mut iTemp60: i32 = (if ((iTemp2) != 0) { 1i32 } else { (if ((iTemp0) != 0) { 0i32 } else { self.iRec16090_5[(1i32) as usize] }) });
            let mut iTemp61: i32 = (((((((iTemp59 < 3i32)) as i32) | ((((iTemp59 < 4i32)) as i32) & (1i32).wrapping_sub(iTemp60))) >= 1i32)) as i32);
            let mut iTemp62: i32 = iTbl1047[(iSlow17) as usize];
            let mut fTemp17: f32 = ((iTemp62) as f32);
            let mut fTemp18: f32 = self.fRec16090[(1i32) as usize];
            let mut iTemp63: i32 = iTbl1047[(iSlow94) as usize];
            let mut iTemp64: i32 = (if ((iTemp2) != 0) { (((iTemp63 > iTemp62)) as i32) } else { (if ((iTemp0) != 0) { (((fTemp17 > fTemp18)) as i32) } else { self.iRec16090_3[(1i32) as usize] }) });
            let mut iTemp65: i32 = (((iTemp64 >= 1i32)) as i32);
            let mut fTemp19: f32 = (if ((iTemp2) != 0) { fTemp17 } else { fTemp18 });
            let mut fTemp20: f32 = (if ((iTemp2) != 0) { (self.fConst4 * ((iTbl1102[(iSlow96) as usize]) as f32)) } else { (if ((iTemp0) != 0) { (self.fConst4 * ((iTbl1102[(iSlow95) as usize]) as f32)) } else { self.fRec16090_4[(1i32) as usize] }) });
            let mut fTemp21: f32 = (fTemp19 - fTemp20);
            let mut iTemp66: i32 = (if ((iTemp2) != 0) { iTemp63 } else { (if ((iTemp0) != 0) { iTemp62 } else { self.iRec16090_2[(1i32) as usize] }) });
            let mut fTemp22: f32 = ((iTemp66) as f32);
            let mut iTemp67: i32 = ((((((fTemp21 <= fTemp22)) as i32) >= 1i32)) as i32);
            let mut iTemp68: i32 = (iTemp59).wrapping_add(1i32);
            let mut fTemp23: f32 = (fTemp19 + fTemp20);
            let mut iTemp69: i32 = ((((((fTemp23 >= fTemp22)) as i32) >= 1i32)) as i32);
            let mut iTemp70: i32 = ((((((iTemp68 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp71: i32 = (((iTemp68 >= 2i32)) as i32);
            let mut iTemp72: i32 = (((iTemp68 >= 1i32)) as i32);
            let mut iTemp73: i32 = (((iTemp68 >= 3i32)) as i32);
            let mut iTemp74: i32 = iTbl1047[(((f32::round((if ((iTemp71) != 0) { (if ((iTemp73) != 0) { fSlow39 } else { fSlow7 }) } else { (if ((iTemp72) != 0) { fSlow6 } else { fSlow85 }) }))) as i32)) as usize];
            let mut iTemp75: i32 = (if ((iTemp70) != 0) { iTemp74 } else { iTemp66 });
            let mut iTemp76: i32 = (if ((iTemp70) != 0) { (((iTemp74 > iTemp66)) as i32) } else { iTemp64 });
            let mut fTemp24: f32 = (if ((iTemp70) != 0) { (self.fConst4 * ((iTbl1102[(((f32::round((if ((iTemp71) != 0) { (if ((iTemp73) != 0) { fSlow86 } else { fSlow9 }) } else { (if ((iTemp72) != 0) { fSlow8 } else { fSlow87 }) }))) as i32)) as usize]) as f32)) } else { fTemp20 });
            let mut iTemp77: i32 = (if ((iSlow22) != 0) { iSlow184 } else { ((((fSlow93 * ((iTbl129[(i32::max(i32::min(32i32, iSlow183), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut iTemp78: i32 = (if ((iSlow24) != 0) { iSlow186 } else { ((((fSlow95 * ((iTbl129[(i32::max(i32::min(32i32, iSlow185), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut fTemp25: f32 = f32::max((((((((((fSlow12 * fTemp1) + 7.0f32)) as i32)).wrapping_shr((3i32) as u32)).wrapping_shl((4i32) as u32)) as f32) + (32.0f32 * f32::min(((if ((iSlow19) != 0) { fSlow91 } else { ((iTbl59[(i32::min(iSlow20, 19i32)) as usize]) as f32) }) + (((if ((iSlow182) != 0) { (if ((iSlow23) != 0) { (-1i32).wrapping_mul(iTemp78) } else { iTemp78 }) } else { (if ((iSlow21) != 0) { (-1i32).wrapping_mul(iTemp77) } else { iTemp77 }) })) as f32)), 127.0f32))), 0.0f32);
            let mut iTemp79: i32 = (((f32::max((((((((((if ((iSlow97) != 0) { fSlow90 } else { ((iTbl59[(i32::min(iSlow98, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp25) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut fTemp26: f32 = (if ((iSlow101) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow104), 0i32)) as usize]) as f32) } else { fSlow100 });
            let mut iTemp80: i32 = (((f32::max((((((((((if ((iSlow105) != 0) { fSlow102 } else { ((iTbl59[(i32::min(iSlow106, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp25) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp81: i32 = (if ((iTemp2) != 0) { 0i32 } else { self.iRec16339[(1i32) as usize] });
            let mut iTemp82: i32 = (if ((iTemp0) != 0) { (if (iTemp80 == iTemp81) { (((self.fConst1 * (if ((iSlow107) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow108), 0i32)) as usize]) as f32) } else { fSlow105 }))) as i32) } else { 0i32 }) } else { (if ((iTemp2) != 0) { (if ((((((iTemp79 == 0i32)) as i32) | iSlow102)) != 0) { (((self.fConst1 * (if ((iSlow103) != 0) { (0.05000000074505806f32 * fTemp26) } else { fTemp26 }))) as i32) } else { 0i32 }) } else { self.iRec16339_6[(1i32) as usize] }) });
            let mut iTemp83: i32 = (((iTemp82 != 0i32)) as i32);
            let mut iTemp84: i32 = ((((iTemp83 & (((iTemp82 <= 1i32)) as i32)) >= 1i32)) as i32);
            let mut iTemp85: i32 = (if ((iTemp0) != 0) { 3i32 } else { (if ((iTemp2) != 0) { 0i32 } else { self.iRec16339_1[(1i32) as usize] }) });
            let mut iTemp86: i32 = (iTemp85).wrapping_add(1i32);
            let mut iTemp87: i32 = (if ((iTemp84) != 0) { iTemp86 } else { iTemp85 });
            let mut iTemp88: i32 = (if ((iTemp0) != 0) { 0i32 } else { (if ((iTemp2) != 0) { 1i32 } else { self.iRec16339_5[(1i32) as usize] }) });
            let mut iTemp89: i32 = (((((((iTemp87 < 3i32)) as i32) | ((((iTemp87 < 4i32)) as i32) & (iTemp88 ^ -1i32))) >= 1i32)) as i32);
            let mut iTemp90: i32 = ((((((iTemp86 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp91: i32 = (((iTemp86 >= 2i32)) as i32);
            let mut iTemp92: i32 = (((iTemp86 >= 1i32)) as i32);
            let mut iTemp93: i32 = (((iTemp86 >= 3i32)) as i32);
            let mut fTemp27: f32 = (if ((iTemp91) != 0) { (if ((iTemp93) != 0) { fSlow101 } else { fSlow11 }) } else { (if ((iTemp92) != 0) { fSlow10 } else { fSlow89 }) });
            let mut iTemp94: i32 = (((f32::max((((((((((if (fTemp27 >= 20.0f32) { (fTemp27 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp27)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp25) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp95: i32 = (((iTemp86 == 0i32)) as i32);
            let mut iTemp96: i32 = (((fTemp27 == 0.0f32)) as i32);
            let mut fTemp28: f32 = (if ((iTemp91) != 0) { (if ((iTemp93) != 0) { fSlow103 } else { fSlow14 }) } else { (if ((iTemp92) != 0) { fSlow13 } else { fSlow96 }) });
            let mut fTemp29: f32 = f32::min((fSlow98 + fTemp28), 99.0f32);
            let mut iTemp97: i32 = (((fTemp29 < 77.0f32)) as i32);
            let mut fTemp30: f32 = (if ((iTemp97) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp29)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp29)) });
            let mut iTemp98: i32 = (if ((iTemp84) != 0) { (if ((iTemp90) != 0) { (if ((((((iTemp94 == iTemp81)) as i32) | (iTemp95 & iTemp96))) != 0) { (((self.fConst1 * (if ((((iTemp97 & iTemp95) & iTemp96)) != 0) { (0.05000000074505806f32 * fTemp30) } else { fTemp30 }))) as i32) } else { 0i32 }) } else { 0i32 }) } else { (iTemp82).wrapping_sub((if ((iTemp83) != 0) { 1i32 } else { 0i32 })) });
            let mut iTemp99: i32 = (if ((iTemp0) != 0) { (((iTemp80 > iTemp81)) as i32) } else { (if ((iTemp2) != 0) { (((iTemp79 > 0i32)) as i32) } else { self.iRec16339_3[(1i32) as usize] }) });
            let mut iTemp100: i32 = (if ((iTemp84) != 0) { (if ((iTemp90) != 0) { (((iTemp94 > iTemp81)) as i32) } else { iTemp99 }) } else { iTemp99 });
            let mut iTemp101: i32 = ((((iTemp98 == 0i32)) as i32)).wrapping_mul(((((iTemp100 == 0i32)) as i32)).wrapping_add(1i32));
            let mut iTemp102: i32 = (((iTemp101 >= 2i32)) as i32);
            let mut iTemp103: i32 = (((iTemp101 >= 1i32)) as i32);
            let mut iTemp104: i32 = i32::max(112459776i32, iTemp81);
            let mut iTemp105: i32 = (if ((iTemp0) != 0) { iSlow112 } else { (if ((iTemp2) != 0) { iSlow110 } else { self.iRec16339_4[(1i32) as usize] }) });
            let mut iTemp106: i32 = i32::min((iSlow100).wrapping_add(((41i32).wrapping_mul(((fTemp28) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp107: i32 = (if ((iTemp84) != 0) { (if ((iTemp90) != 0) { (((self.fConst1 * (((((iTemp106 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp106).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp105 }) } else { iTemp105 });
            let mut iTemp108: i32 = (iTemp104).wrapping_add((((285212672i32).wrapping_sub(iTemp104)).wrapping_shr((24i32) as u32)).wrapping_mul(iTemp107));
            let mut iTemp109: i32 = (if ((iTemp0) != 0) { iTemp80 } else { (if ((iTemp2) != 0) { iTemp79 } else { self.iRec16339_2[(1i32) as usize] }) });
            let mut iTemp110: i32 = (if ((iTemp84) != 0) { (if ((iTemp90) != 0) { iTemp94 } else { iTemp109 }) } else { iTemp109 });
            let mut iTemp111: i32 = ((((((iTemp108 >= iTemp110)) as i32) >= 1i32)) as i32);
            let mut iTemp112: i32 = (iTemp87).wrapping_add(1i32);
            let mut iTemp113: i32 = (iTemp81).wrapping_sub(iTemp107);
            let mut iTemp114: i32 = ((((((iTemp113 <= iTemp110)) as i32) >= 1i32)) as i32);
            let mut iTemp115: i32 = ((((((iTemp112 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp116: i32 = (((iTemp112 >= 2i32)) as i32);
            let mut iTemp117: i32 = (((iTemp112 >= 1i32)) as i32);
            let mut iTemp118: i32 = (((iTemp112 >= 3i32)) as i32);
            let mut fTemp31: f32 = (if ((iTemp116) != 0) { (if ((iTemp118) != 0) { fSlow101 } else { fSlow11 }) } else { (if ((iTemp117) != 0) { fSlow10 } else { fSlow89 }) });
            let mut iTemp119: i32 = (((f32::max(((fTemp25 + (((((((if (fTemp31 >= 20.0f32) { (fTemp31 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp31)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32)) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp120: i32 = (if ((iTemp115) != 0) { iTemp119 } else { iTemp110 });
            let mut iTemp121: i32 = (if ((iTemp115) != 0) { (((iTemp119 > iTemp110)) as i32) } else { iTemp100 });
            let mut fTemp32: f32 = (if ((iTemp116) != 0) { (if ((iTemp118) != 0) { fSlow103 } else { fSlow14 }) } else { (if ((iTemp117) != 0) { fSlow13 } else { fSlow96 }) });
            let mut iTemp122: i32 = i32::min((iSlow100).wrapping_add(((41i32).wrapping_mul(((fTemp32) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp123: i32 = (if ((iTemp115) != 0) { (((self.fConst1 * (((((iTemp122 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp122).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp107 });
            let mut iTemp124: i32 = (((iTemp112 == 0i32)) as i32);
            let mut iTemp125: i32 = (((fTemp31 == 0.0f32)) as i32);
            let mut fTemp33: f32 = f32::min((fSlow98 + fTemp32), 99.0f32);
            let mut iTemp126: i32 = (((fTemp33 < 77.0f32)) as i32);
            let mut fTemp34: f32 = (if ((iTemp126) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp33)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp33)) });
            let mut iTemp127: i32 = (if ((iTemp115) != 0) { (if ((((((iTemp119 == iTemp110)) as i32) | (iTemp124 & iTemp125))) != 0) { (((self.fConst1 * (if ((((iTemp126 & iTemp124) & iTemp125)) != 0) { (0.05000000074505806f32 * fTemp34) } else { fTemp34 }))) as i32) } else { 0i32 }) } else { iTemp98 });
            let mut iTemp128: i32 = (if ((iSlow32) != 0) { iSlow189 } else { ((((fSlow114 * ((iTbl129[(i32::max(i32::min(32i32, iSlow188), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut iTemp129: i32 = (if ((iSlow34) != 0) { iSlow191 } else { ((((fSlow116 * ((iTbl129[(i32::max(i32::min(32i32, iSlow190), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut fTemp35: f32 = f32::max((((((((((fSlow17 * fTemp1) + 7.0f32)) as i32)).wrapping_shr((3i32) as u32)).wrapping_shl((4i32) as u32)) as f32) + (32.0f32 * f32::min(((if ((iSlow29) != 0) { fSlow112 } else { ((iTbl59[(i32::min(iSlow30, 19i32)) as usize]) as f32) }) + (((if ((iSlow187) != 0) { (if ((iSlow33) != 0) { (-1i32).wrapping_mul(iTemp129) } else { iTemp129 }) } else { (if ((iSlow31) != 0) { (-1i32).wrapping_mul(iTemp128) } else { iTemp128 }) })) as f32)), 127.0f32))), 0.0f32);
            let mut iTemp130: i32 = (((f32::max((((((((((if ((iSlow113) != 0) { fSlow111 } else { ((iTbl59[(i32::min(iSlow114, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp35) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut fTemp36: f32 = (if ((iSlow117) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow120), 0i32)) as usize]) as f32) } else { fSlow121 });
            let mut iTemp131: i32 = (((f32::max((((((((((if ((iSlow121) != 0) { fSlow123 } else { ((iTbl59[(i32::min(iSlow122, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp35) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp132: i32 = (if ((iTemp2) != 0) { 0i32 } else { self.iRec16599[(1i32) as usize] });
            let mut iTemp133: i32 = (if ((iTemp0) != 0) { (if (iTemp131 == iTemp132) { (((self.fConst1 * (if ((iSlow123) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow124), 0i32)) as usize]) as f32) } else { fSlow126 }))) as i32) } else { 0i32 }) } else { (if ((iTemp2) != 0) { (if ((((((iTemp130 == 0i32)) as i32) | iSlow118)) != 0) { (((self.fConst1 * (if ((iSlow119) != 0) { (0.05000000074505806f32 * fTemp36) } else { fTemp36 }))) as i32) } else { 0i32 }) } else { self.iRec16599_6[(1i32) as usize] }) });
            let mut iTemp134: i32 = (((iTemp133 != 0i32)) as i32);
            let mut iTemp135: i32 = ((((iTemp134 & (((iTemp133 <= 1i32)) as i32)) >= 1i32)) as i32);
            let mut iTemp136: i32 = (if ((iTemp0) != 0) { 3i32 } else { (if ((iTemp2) != 0) { 0i32 } else { self.iRec16599_1[(1i32) as usize] }) });
            let mut iTemp137: i32 = (iTemp136).wrapping_add(1i32);
            let mut iTemp138: i32 = (if ((iTemp135) != 0) { iTemp137 } else { iTemp136 });
            let mut iTemp139: i32 = (if ((iTemp0) != 0) { 0i32 } else { (if ((iTemp2) != 0) { 1i32 } else { self.iRec16599_5[(1i32) as usize] }) });
            let mut iTemp140: i32 = (((((((iTemp138 < 3i32)) as i32) | ((((iTemp138 < 4i32)) as i32) & (iTemp139 ^ -1i32))) >= 1i32)) as i32);
            let mut iTemp141: i32 = ((((((iTemp137 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp142: i32 = (((iTemp137 >= 2i32)) as i32);
            let mut iTemp143: i32 = (((iTemp137 >= 1i32)) as i32);
            let mut iTemp144: i32 = (((iTemp137 >= 3i32)) as i32);
            let mut fTemp37: f32 = (if ((iTemp142) != 0) { (if ((iTemp144) != 0) { fSlow122 } else { fSlow16 }) } else { (if ((iTemp143) != 0) { fSlow15 } else { fSlow110 }) });
            let mut iTemp145: i32 = (((f32::max((((((((((if (fTemp37 >= 20.0f32) { (fTemp37 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp37)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp35) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp146: i32 = (((iTemp137 == 0i32)) as i32);
            let mut iTemp147: i32 = (((fTemp37 == 0.0f32)) as i32);
            let mut fTemp38: f32 = (if ((iTemp142) != 0) { (if ((iTemp144) != 0) { fSlow124 } else { fSlow19 }) } else { (if ((iTemp143) != 0) { fSlow18 } else { fSlow117 }) });
            let mut fTemp39: f32 = f32::min((fSlow119 + fTemp38), 99.0f32);
            let mut iTemp148: i32 = (((fTemp39 < 77.0f32)) as i32);
            let mut fTemp40: f32 = (if ((iTemp148) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp39)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp39)) });
            let mut iTemp149: i32 = (if ((iTemp135) != 0) { (if ((iTemp141) != 0) { (if ((((((iTemp145 == iTemp132)) as i32) | (iTemp146 & iTemp147))) != 0) { (((self.fConst1 * (if ((((iTemp148 & iTemp146) & iTemp147)) != 0) { (0.05000000074505806f32 * fTemp40) } else { fTemp40 }))) as i32) } else { 0i32 }) } else { 0i32 }) } else { (iTemp133).wrapping_sub((if ((iTemp134) != 0) { 1i32 } else { 0i32 })) });
            let mut iTemp150: i32 = (if ((iTemp0) != 0) { (((iTemp131 > iTemp132)) as i32) } else { (if ((iTemp2) != 0) { (((iTemp130 > 0i32)) as i32) } else { self.iRec16599_3[(1i32) as usize] }) });
            let mut iTemp151: i32 = (if ((iTemp135) != 0) { (if ((iTemp141) != 0) { (((iTemp145 > iTemp132)) as i32) } else { iTemp150 }) } else { iTemp150 });
            let mut iTemp152: i32 = ((((iTemp149 == 0i32)) as i32)).wrapping_mul(((((iTemp151 == 0i32)) as i32)).wrapping_add(1i32));
            let mut iTemp153: i32 = (((iTemp152 >= 2i32)) as i32);
            let mut iTemp154: i32 = (((iTemp152 >= 1i32)) as i32);
            let mut iTemp155: i32 = i32::max(112459776i32, iTemp132);
            let mut iTemp156: i32 = (if ((iTemp0) != 0) { iSlow128 } else { (if ((iTemp2) != 0) { iSlow126 } else { self.iRec16599_4[(1i32) as usize] }) });
            let mut iTemp157: i32 = i32::min((iSlow116).wrapping_add(((41i32).wrapping_mul(((fTemp38) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp158: i32 = (if ((iTemp135) != 0) { (if ((iTemp141) != 0) { (((self.fConst1 * (((((iTemp157 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp157).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp156 }) } else { iTemp156 });
            let mut iTemp159: i32 = (iTemp155).wrapping_add((((285212672i32).wrapping_sub(iTemp155)).wrapping_shr((24i32) as u32)).wrapping_mul(iTemp158));
            let mut iTemp160: i32 = (if ((iTemp0) != 0) { iTemp131 } else { (if ((iTemp2) != 0) { iTemp130 } else { self.iRec16599_2[(1i32) as usize] }) });
            let mut iTemp161: i32 = (if ((iTemp135) != 0) { (if ((iTemp141) != 0) { iTemp145 } else { iTemp160 }) } else { iTemp160 });
            let mut iTemp162: i32 = ((((((iTemp159 >= iTemp161)) as i32) >= 1i32)) as i32);
            let mut iTemp163: i32 = (iTemp138).wrapping_add(1i32);
            let mut iTemp164: i32 = (iTemp132).wrapping_sub(iTemp158);
            let mut iTemp165: i32 = ((((((iTemp164 <= iTemp161)) as i32) >= 1i32)) as i32);
            let mut iTemp166: i32 = ((((((iTemp163 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp167: i32 = (((iTemp163 >= 2i32)) as i32);
            let mut iTemp168: i32 = (((iTemp163 >= 1i32)) as i32);
            let mut iTemp169: i32 = (((iTemp163 >= 3i32)) as i32);
            let mut fTemp41: f32 = (if ((iTemp167) != 0) { (if ((iTemp169) != 0) { fSlow122 } else { fSlow16 }) } else { (if ((iTemp168) != 0) { fSlow15 } else { fSlow110 }) });
            let mut iTemp170: i32 = (((f32::max(((fTemp35 + (((((((if (fTemp41 >= 20.0f32) { (fTemp41 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp41)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32)) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp171: i32 = (if ((iTemp166) != 0) { iTemp170 } else { iTemp161 });
            let mut iTemp172: i32 = (if ((iTemp166) != 0) { (((iTemp170 > iTemp161)) as i32) } else { iTemp151 });
            let mut fTemp42: f32 = (if ((iTemp167) != 0) { (if ((iTemp169) != 0) { fSlow124 } else { fSlow19 }) } else { (if ((iTemp168) != 0) { fSlow18 } else { fSlow117 }) });
            let mut iTemp173: i32 = i32::min((iSlow116).wrapping_add(((41i32).wrapping_mul(((fTemp42) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp174: i32 = (if ((iTemp166) != 0) { (((self.fConst1 * (((((iTemp173 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp173).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp158 });
            let mut iTemp175: i32 = (((iTemp163 == 0i32)) as i32);
            let mut iTemp176: i32 = (((fTemp41 == 0.0f32)) as i32);
            let mut fTemp43: f32 = f32::min((fSlow119 + fTemp42), 99.0f32);
            let mut iTemp177: i32 = (((fTemp43 < 77.0f32)) as i32);
            let mut fTemp44: f32 = (if ((iTemp177) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp43)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp43)) });
            let mut iTemp178: i32 = (if ((iTemp166) != 0) { (if ((((((iTemp170 == iTemp161)) as i32) | (iTemp175 & iTemp176))) != 0) { (((self.fConst1 * (if ((((iTemp177 & iTemp175) & iTemp176)) != 0) { (0.05000000074505806f32 * fTemp44) } else { fTemp44 }))) as i32) } else { 0i32 }) } else { iTemp149 });
            let mut iTemp179: i32 = (if ((iSlow42) != 0) { iSlow194 } else { ((((fSlow135 * ((iTbl129[(i32::max(i32::min(32i32, iSlow193), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut iTemp180: i32 = (if ((iSlow44) != 0) { iSlow196 } else { ((((fSlow137 * ((iTbl129[(i32::max(i32::min(32i32, iSlow195), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut fTemp45: f32 = f32::max((((((((((fSlow22 * fTemp1) + 7.0f32)) as i32)).wrapping_shr((3i32) as u32)).wrapping_shl((4i32) as u32)) as f32) + (32.0f32 * f32::min(((if ((iSlow39) != 0) { fSlow133 } else { ((iTbl59[(i32::min(iSlow40, 19i32)) as usize]) as f32) }) + (((if ((iSlow192) != 0) { (if ((iSlow43) != 0) { (-1i32).wrapping_mul(iTemp180) } else { iTemp180 }) } else { (if ((iSlow41) != 0) { (-1i32).wrapping_mul(iTemp179) } else { iTemp179 }) })) as f32)), 127.0f32))), 0.0f32);
            let mut iTemp181: i32 = (((f32::max((((((((((if ((iSlow129) != 0) { fSlow132 } else { ((iTbl59[(i32::min(iSlow130, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp45) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut fTemp46: f32 = (if ((iSlow133) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow136), 0i32)) as usize]) as f32) } else { fSlow142 });
            let mut iTemp182: i32 = (((f32::max((((((((((if ((iSlow137) != 0) { fSlow144 } else { ((iTbl59[(i32::min(iSlow138, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp45) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp183: i32 = (if ((iTemp2) != 0) { 0i32 } else { self.iRec16851[(1i32) as usize] });
            let mut iTemp184: i32 = (if ((iTemp0) != 0) { (if (iTemp182 == iTemp183) { (((self.fConst1 * (if ((iSlow139) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow140), 0i32)) as usize]) as f32) } else { fSlow147 }))) as i32) } else { 0i32 }) } else { (if ((iTemp2) != 0) { (if ((((((iTemp181 == 0i32)) as i32) | iSlow134)) != 0) { (((self.fConst1 * (if ((iSlow135) != 0) { (0.05000000074505806f32 * fTemp46) } else { fTemp46 }))) as i32) } else { 0i32 }) } else { self.iRec16851_6[(1i32) as usize] }) });
            let mut iTemp185: i32 = (((iTemp184 != 0i32)) as i32);
            let mut iTemp186: i32 = ((((iTemp185 & (((iTemp184 <= 1i32)) as i32)) >= 1i32)) as i32);
            let mut iTemp187: i32 = (if ((iTemp0) != 0) { 3i32 } else { (if ((iTemp2) != 0) { 0i32 } else { self.iRec16851_1[(1i32) as usize] }) });
            let mut iTemp188: i32 = (iTemp187).wrapping_add(1i32);
            let mut iTemp189: i32 = (if ((iTemp186) != 0) { iTemp188 } else { iTemp187 });
            let mut iTemp190: i32 = (if ((iTemp0) != 0) { 0i32 } else { (if ((iTemp2) != 0) { 1i32 } else { self.iRec16851_5[(1i32) as usize] }) });
            let mut iTemp191: i32 = (((((((iTemp189 < 3i32)) as i32) | ((((iTemp189 < 4i32)) as i32) & (iTemp190 ^ -1i32))) >= 1i32)) as i32);
            let mut iTemp192: i32 = ((((((iTemp188 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp193: i32 = (((iTemp188 >= 2i32)) as i32);
            let mut iTemp194: i32 = (((iTemp188 >= 1i32)) as i32);
            let mut iTemp195: i32 = (((iTemp188 >= 3i32)) as i32);
            let mut fTemp47: f32 = (if ((iTemp193) != 0) { (if ((iTemp195) != 0) { fSlow143 } else { fSlow21 }) } else { (if ((iTemp194) != 0) { fSlow20 } else { fSlow131 }) });
            let mut iTemp196: i32 = (((f32::max((((((((((if (fTemp47 >= 20.0f32) { (fTemp47 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp47)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp45) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp197: i32 = (((iTemp188 == 0i32)) as i32);
            let mut iTemp198: i32 = (((fTemp47 == 0.0f32)) as i32);
            let mut fTemp48: f32 = (if ((iTemp193) != 0) { (if ((iTemp195) != 0) { fSlow145 } else { fSlow24 }) } else { (if ((iTemp194) != 0) { fSlow23 } else { fSlow138 }) });
            let mut fTemp49: f32 = f32::min((fSlow140 + fTemp48), 99.0f32);
            let mut iTemp199: i32 = (((fTemp49 < 77.0f32)) as i32);
            let mut fTemp50: f32 = (if ((iTemp199) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp49)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp49)) });
            let mut iTemp200: i32 = (if ((iTemp186) != 0) { (if ((iTemp192) != 0) { (if ((((((iTemp196 == iTemp183)) as i32) | (iTemp197 & iTemp198))) != 0) { (((self.fConst1 * (if ((((iTemp199 & iTemp197) & iTemp198)) != 0) { (0.05000000074505806f32 * fTemp50) } else { fTemp50 }))) as i32) } else { 0i32 }) } else { 0i32 }) } else { (iTemp184).wrapping_sub((if ((iTemp185) != 0) { 1i32 } else { 0i32 })) });
            let mut iTemp201: i32 = (if ((iTemp0) != 0) { (((iTemp182 > iTemp183)) as i32) } else { (if ((iTemp2) != 0) { (((iTemp181 > 0i32)) as i32) } else { self.iRec16851_3[(1i32) as usize] }) });
            let mut iTemp202: i32 = (if ((iTemp186) != 0) { (if ((iTemp192) != 0) { (((iTemp196 > iTemp183)) as i32) } else { iTemp201 }) } else { iTemp201 });
            let mut iTemp203: i32 = ((((iTemp200 == 0i32)) as i32)).wrapping_mul(((((iTemp202 == 0i32)) as i32)).wrapping_add(1i32));
            let mut iTemp204: i32 = (((iTemp203 >= 2i32)) as i32);
            let mut iTemp205: i32 = (((iTemp203 >= 1i32)) as i32);
            let mut iTemp206: i32 = i32::max(112459776i32, iTemp183);
            let mut iTemp207: i32 = (if ((iTemp0) != 0) { iSlow144 } else { (if ((iTemp2) != 0) { iSlow142 } else { self.iRec16851_4[(1i32) as usize] }) });
            let mut iTemp208: i32 = i32::min((iSlow132).wrapping_add(((41i32).wrapping_mul(((fTemp48) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp209: i32 = (if ((iTemp186) != 0) { (if ((iTemp192) != 0) { (((self.fConst1 * (((((iTemp208 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp208).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp207 }) } else { iTemp207 });
            let mut iTemp210: i32 = (iTemp206).wrapping_add((((285212672i32).wrapping_sub(iTemp206)).wrapping_shr((24i32) as u32)).wrapping_mul(iTemp209));
            let mut iTemp211: i32 = (if ((iTemp0) != 0) { iTemp182 } else { (if ((iTemp2) != 0) { iTemp181 } else { self.iRec16851_2[(1i32) as usize] }) });
            let mut iTemp212: i32 = (if ((iTemp186) != 0) { (if ((iTemp192) != 0) { iTemp196 } else { iTemp211 }) } else { iTemp211 });
            let mut iTemp213: i32 = ((((((iTemp210 >= iTemp212)) as i32) >= 1i32)) as i32);
            let mut iTemp214: i32 = (iTemp189).wrapping_add(1i32);
            let mut iTemp215: i32 = (iTemp183).wrapping_sub(iTemp209);
            let mut iTemp216: i32 = ((((((iTemp215 <= iTemp212)) as i32) >= 1i32)) as i32);
            let mut iTemp217: i32 = ((((((iTemp214 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp218: i32 = (((iTemp214 >= 2i32)) as i32);
            let mut iTemp219: i32 = (((iTemp214 >= 1i32)) as i32);
            let mut iTemp220: i32 = (((iTemp214 >= 3i32)) as i32);
            let mut fTemp51: f32 = (if ((iTemp218) != 0) { (if ((iTemp220) != 0) { fSlow143 } else { fSlow21 }) } else { (if ((iTemp219) != 0) { fSlow20 } else { fSlow131 }) });
            let mut iTemp221: i32 = (((f32::max(((fTemp45 + (((((((if (fTemp51 >= 20.0f32) { (fTemp51 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp51)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32)) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp222: i32 = (if ((iTemp217) != 0) { iTemp221 } else { iTemp212 });
            let mut iTemp223: i32 = (if ((iTemp217) != 0) { (((iTemp221 > iTemp212)) as i32) } else { iTemp202 });
            let mut fTemp52: f32 = (if ((iTemp218) != 0) { (if ((iTemp220) != 0) { fSlow145 } else { fSlow24 }) } else { (if ((iTemp219) != 0) { fSlow23 } else { fSlow138 }) });
            let mut iTemp224: i32 = i32::min((iSlow132).wrapping_add(((41i32).wrapping_mul(((fTemp52) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp225: i32 = (if ((iTemp217) != 0) { (((self.fConst1 * (((((iTemp224 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp224).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp209 });
            let mut iTemp226: i32 = (((iTemp214 == 0i32)) as i32);
            let mut iTemp227: i32 = (((fTemp51 == 0.0f32)) as i32);
            let mut fTemp53: f32 = f32::min((fSlow140 + fTemp52), 99.0f32);
            let mut iTemp228: i32 = (((fTemp53 < 77.0f32)) as i32);
            let mut fTemp54: f32 = (if ((iTemp228) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp53)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp53)) });
            let mut iTemp229: i32 = (if ((iTemp217) != 0) { (if ((((((iTemp221 == iTemp212)) as i32) | (iTemp226 & iTemp227))) != 0) { (((self.fConst1 * (if ((((iTemp228 & iTemp226) & iTemp227)) != 0) { (0.05000000074505806f32 * fTemp54) } else { fTemp54 }))) as i32) } else { 0i32 }) } else { iTemp200 });
            let mut iTemp230: i32 = (if ((iSlow52) != 0) { iSlow199 } else { ((((fSlow156 * ((iTbl129[(i32::max(i32::min(32i32, iSlow198), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut iTemp231: i32 = (if ((iSlow54) != 0) { iSlow201 } else { ((((fSlow158 * ((iTbl129[(i32::max(i32::min(32i32, iSlow200), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut fTemp55: f32 = f32::max((((((((((fSlow27 * fTemp1) + 7.0f32)) as i32)).wrapping_shr((3i32) as u32)).wrapping_shl((4i32) as u32)) as f32) + (32.0f32 * f32::min(((if ((iSlow49) != 0) { fSlow154 } else { ((iTbl59[(i32::min(iSlow50, 19i32)) as usize]) as f32) }) + (((if ((iSlow197) != 0) { (if ((iSlow53) != 0) { (-1i32).wrapping_mul(iTemp231) } else { iTemp231 }) } else { (if ((iSlow51) != 0) { (-1i32).wrapping_mul(iTemp230) } else { iTemp230 }) })) as f32)), 127.0f32))), 0.0f32);
            let mut iTemp232: i32 = (((f32::max((((((((((if ((iSlow145) != 0) { fSlow153 } else { ((iTbl59[(i32::min(iSlow146, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp55) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut fTemp56: f32 = (if ((iSlow149) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow152), 0i32)) as usize]) as f32) } else { fSlow163 });
            let mut iTemp233: i32 = (((f32::max((((((((((if ((iSlow153) != 0) { fSlow165 } else { ((iTbl59[(i32::min(iSlow154, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp55) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp234: i32 = (if ((iTemp2) != 0) { 0i32 } else { self.iRec17112[(1i32) as usize] });
            let mut iTemp235: i32 = (if ((iTemp0) != 0) { (if (iTemp233 == iTemp234) { (((self.fConst1 * (if ((iSlow155) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow156), 0i32)) as usize]) as f32) } else { fSlow168 }))) as i32) } else { 0i32 }) } else { (if ((iTemp2) != 0) { (if ((((((iTemp232 == 0i32)) as i32) | iSlow150)) != 0) { (((self.fConst1 * (if ((iSlow151) != 0) { (0.05000000074505806f32 * fTemp56) } else { fTemp56 }))) as i32) } else { 0i32 }) } else { self.iRec17112_6[(1i32) as usize] }) });
            let mut iTemp236: i32 = (((iTemp235 != 0i32)) as i32);
            let mut iTemp237: i32 = ((((iTemp236 & (((iTemp235 <= 1i32)) as i32)) >= 1i32)) as i32);
            let mut iTemp238: i32 = (if ((iTemp0) != 0) { 3i32 } else { (if ((iTemp2) != 0) { 0i32 } else { self.iRec17112_1[(1i32) as usize] }) });
            let mut iTemp239: i32 = (iTemp238).wrapping_add(1i32);
            let mut iTemp240: i32 = (if ((iTemp237) != 0) { iTemp239 } else { iTemp238 });
            let mut iTemp241: i32 = (if ((iTemp0) != 0) { 0i32 } else { (if ((iTemp2) != 0) { 1i32 } else { self.iRec17112_5[(1i32) as usize] }) });
            let mut iTemp242: i32 = (((((((iTemp240 < 3i32)) as i32) | ((((iTemp240 < 4i32)) as i32) & (iTemp241 ^ -1i32))) >= 1i32)) as i32);
            let mut iTemp243: i32 = ((((((iTemp239 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp244: i32 = (((iTemp239 >= 2i32)) as i32);
            let mut iTemp245: i32 = (((iTemp239 >= 1i32)) as i32);
            let mut iTemp246: i32 = (((iTemp239 >= 3i32)) as i32);
            let mut fTemp57: f32 = (if ((iTemp244) != 0) { (if ((iTemp246) != 0) { fSlow164 } else { fSlow26 }) } else { (if ((iTemp245) != 0) { fSlow25 } else { fSlow152 }) });
            let mut iTemp247: i32 = (((f32::max((((((((((if (fTemp57 >= 20.0f32) { (fTemp57 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp57)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp55) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp248: i32 = (((iTemp239 == 0i32)) as i32);
            let mut iTemp249: i32 = (((fTemp57 == 0.0f32)) as i32);
            let mut fTemp58: f32 = (if ((iTemp244) != 0) { (if ((iTemp246) != 0) { fSlow166 } else { fSlow29 }) } else { (if ((iTemp245) != 0) { fSlow28 } else { fSlow159 }) });
            let mut fTemp59: f32 = f32::min((fSlow161 + fTemp58), 99.0f32);
            let mut iTemp250: i32 = (((fTemp59 < 77.0f32)) as i32);
            let mut fTemp60: f32 = (if ((iTemp250) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp59)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp59)) });
            let mut iTemp251: i32 = (if ((iTemp237) != 0) { (if ((iTemp243) != 0) { (if ((((((iTemp247 == iTemp234)) as i32) | (iTemp248 & iTemp249))) != 0) { (((self.fConst1 * (if ((((iTemp250 & iTemp248) & iTemp249)) != 0) { (0.05000000074505806f32 * fTemp60) } else { fTemp60 }))) as i32) } else { 0i32 }) } else { 0i32 }) } else { (iTemp235).wrapping_sub((if ((iTemp236) != 0) { 1i32 } else { 0i32 })) });
            let mut iTemp252: i32 = (if ((iTemp0) != 0) { (((iTemp233 > iTemp234)) as i32) } else { (if ((iTemp2) != 0) { (((iTemp232 > 0i32)) as i32) } else { self.iRec17112_3[(1i32) as usize] }) });
            let mut iTemp253: i32 = (if ((iTemp237) != 0) { (if ((iTemp243) != 0) { (((iTemp247 > iTemp234)) as i32) } else { iTemp252 }) } else { iTemp252 });
            let mut iTemp254: i32 = ((((iTemp251 == 0i32)) as i32)).wrapping_mul(((((iTemp253 == 0i32)) as i32)).wrapping_add(1i32));
            let mut iTemp255: i32 = (((iTemp254 >= 2i32)) as i32);
            let mut iTemp256: i32 = (((iTemp254 >= 1i32)) as i32);
            let mut iTemp257: i32 = i32::max(112459776i32, iTemp234);
            let mut iTemp258: i32 = (if ((iTemp0) != 0) { iSlow160 } else { (if ((iTemp2) != 0) { iSlow158 } else { self.iRec17112_4[(1i32) as usize] }) });
            let mut iTemp259: i32 = i32::min((iSlow148).wrapping_add(((41i32).wrapping_mul(((fTemp58) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp260: i32 = (if ((iTemp237) != 0) { (if ((iTemp243) != 0) { (((self.fConst1 * (((((iTemp259 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp259).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp258 }) } else { iTemp258 });
            let mut iTemp261: i32 = (iTemp257).wrapping_add((((285212672i32).wrapping_sub(iTemp257)).wrapping_shr((24i32) as u32)).wrapping_mul(iTemp260));
            let mut iTemp262: i32 = (if ((iTemp0) != 0) { iTemp233 } else { (if ((iTemp2) != 0) { iTemp232 } else { self.iRec17112_2[(1i32) as usize] }) });
            let mut iTemp263: i32 = (if ((iTemp237) != 0) { (if ((iTemp243) != 0) { iTemp247 } else { iTemp262 }) } else { iTemp262 });
            let mut iTemp264: i32 = ((((((iTemp261 >= iTemp263)) as i32) >= 1i32)) as i32);
            let mut iTemp265: i32 = (iTemp240).wrapping_add(1i32);
            let mut iTemp266: i32 = (iTemp234).wrapping_sub(iTemp260);
            let mut iTemp267: i32 = ((((((iTemp266 <= iTemp263)) as i32) >= 1i32)) as i32);
            let mut iTemp268: i32 = ((((((iTemp265 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp269: i32 = (((iTemp265 >= 2i32)) as i32);
            let mut iTemp270: i32 = (((iTemp265 >= 1i32)) as i32);
            let mut iTemp271: i32 = (((iTemp265 >= 3i32)) as i32);
            let mut fTemp61: f32 = (if ((iTemp269) != 0) { (if ((iTemp271) != 0) { fSlow164 } else { fSlow26 }) } else { (if ((iTemp270) != 0) { fSlow25 } else { fSlow152 }) });
            let mut iTemp272: i32 = (((f32::max(((fTemp55 + (((((((if (fTemp61 >= 20.0f32) { (fTemp61 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp61)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32)) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp273: i32 = (if ((iTemp268) != 0) { iTemp272 } else { iTemp263 });
            let mut iTemp274: i32 = (if ((iTemp268) != 0) { (((iTemp272 > iTemp263)) as i32) } else { iTemp253 });
            let mut fTemp62: f32 = (if ((iTemp269) != 0) { (if ((iTemp271) != 0) { fSlow166 } else { fSlow29 }) } else { (if ((iTemp270) != 0) { fSlow28 } else { fSlow159 }) });
            let mut iTemp275: i32 = i32::min((iSlow148).wrapping_add(((41i32).wrapping_mul(((fTemp62) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp276: i32 = (if ((iTemp268) != 0) { (((self.fConst1 * (((((iTemp275 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp275).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp260 });
            let mut iTemp277: i32 = (((iTemp265 == 0i32)) as i32);
            let mut iTemp278: i32 = (((fTemp61 == 0.0f32)) as i32);
            let mut fTemp63: f32 = f32::min((fSlow161 + fTemp62), 99.0f32);
            let mut iTemp279: i32 = (((fTemp63 < 77.0f32)) as i32);
            let mut fTemp64: f32 = (if ((iTemp279) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp63)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp63)) });
            let mut iTemp280: i32 = (if ((iTemp268) != 0) { (if ((((((iTemp272 == iTemp263)) as i32) | (iTemp277 & iTemp278))) != 0) { (((self.fConst1 * (if ((((iTemp279 & iTemp277) & iTemp278)) != 0) { (0.05000000074505806f32 * fTemp64) } else { fTemp64 }))) as i32) } else { 0i32 }) } else { iTemp251 });
            let mut iTemp281: i32 = (if ((iSlow62) != 0) { iSlow204 } else { ((((fSlow177 * ((iTbl129[(i32::max(i32::min(32i32, iSlow203), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut iTemp282: i32 = (if ((iSlow64) != 0) { iSlow206 } else { ((((fSlow179 * ((iTbl129[(i32::max(i32::min(32i32, iSlow205), 0i32)) as usize]) as f32))) as i32)).wrapping_shr((15i32) as u32) });
            let mut fTemp65: f32 = f32::max((((((((((fSlow32 * fTemp1) + 7.0f32)) as i32)).wrapping_shr((3i32) as u32)).wrapping_shl((4i32) as u32)) as f32) + (32.0f32 * f32::min(((if ((iSlow59) != 0) { fSlow175 } else { ((iTbl59[(i32::min(iSlow60, 19i32)) as usize]) as f32) }) + (((if ((iSlow202) != 0) { (if ((iSlow63) != 0) { (-1i32).wrapping_mul(iTemp282) } else { iTemp282 }) } else { (if ((iSlow61) != 0) { (-1i32).wrapping_mul(iTemp281) } else { iTemp281 }) })) as f32)), 127.0f32))), 0.0f32);
            let mut iTemp283: i32 = (((f32::max((((((((((if ((iSlow161) != 0) { fSlow174 } else { ((iTbl59[(i32::min(iSlow162, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp65) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut fTemp66: f32 = (if ((iSlow165) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow168), 0i32)) as usize]) as f32) } else { fSlow184 });
            let mut iTemp284: i32 = (((f32::max((((((((((if ((iSlow169) != 0) { fSlow186 } else { ((iTbl59[(i32::min(iSlow170, 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp65) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp285: i32 = (if ((iTemp2) != 0) { 0i32 } else { self.iRec17364[(1i32) as usize] });
            let mut iTemp286: i32 = (if ((iTemp0) != 0) { (if (iTemp284 == iTemp285) { (((self.fConst1 * (if ((iSlow171) != 0) { ((iTbl382[(i32::max(i32::min(76i32, iSlow172), 0i32)) as usize]) as f32) } else { fSlow189 }))) as i32) } else { 0i32 }) } else { (if ((iTemp2) != 0) { (if ((((((iTemp283 == 0i32)) as i32) | iSlow166)) != 0) { (((self.fConst1 * (if ((iSlow167) != 0) { (0.05000000074505806f32 * fTemp66) } else { fTemp66 }))) as i32) } else { 0i32 }) } else { self.iRec17364_6[(1i32) as usize] }) });
            let mut iTemp287: i32 = (((iTemp286 != 0i32)) as i32);
            let mut iTemp288: i32 = ((((iTemp287 & (((iTemp286 <= 1i32)) as i32)) >= 1i32)) as i32);
            let mut iTemp289: i32 = (if ((iTemp0) != 0) { 3i32 } else { (if ((iTemp2) != 0) { 0i32 } else { self.iRec17364_1[(1i32) as usize] }) });
            let mut iTemp290: i32 = (iTemp289).wrapping_add(1i32);
            let mut iTemp291: i32 = (if ((iTemp288) != 0) { iTemp290 } else { iTemp289 });
            let mut iTemp292: i32 = (if ((iTemp0) != 0) { 0i32 } else { (if ((iTemp2) != 0) { 1i32 } else { self.iRec17364_5[(1i32) as usize] }) });
            let mut iTemp293: i32 = (((((((iTemp291 < 3i32)) as i32) | ((((iTemp291 < 4i32)) as i32) & (iTemp292 ^ -1i32))) >= 1i32)) as i32);
            let mut iTemp294: i32 = ((((((iTemp290 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp295: i32 = (((iTemp290 >= 2i32)) as i32);
            let mut iTemp296: i32 = (((iTemp290 >= 1i32)) as i32);
            let mut iTemp297: i32 = (((iTemp290 >= 3i32)) as i32);
            let mut fTemp67: f32 = (if ((iTemp295) != 0) { (if ((iTemp297) != 0) { fSlow185 } else { fSlow31 }) } else { (if ((iTemp296) != 0) { fSlow30 } else { fSlow173 }) });
            let mut iTemp298: i32 = (((f32::max((((((((((if (fTemp67 >= 20.0f32) { (fTemp67 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp67)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32) + fTemp65) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp299: i32 = (((iTemp290 == 0i32)) as i32);
            let mut iTemp300: i32 = (((fTemp67 == 0.0f32)) as i32);
            let mut fTemp68: f32 = (if ((iTemp295) != 0) { (if ((iTemp297) != 0) { fSlow187 } else { fSlow34 }) } else { (if ((iTemp296) != 0) { fSlow33 } else { fSlow180 }) });
            let mut fTemp69: f32 = f32::min((fSlow182 + fTemp68), 99.0f32);
            let mut iTemp301: i32 = (((fTemp69 < 77.0f32)) as i32);
            let mut fTemp70: f32 = (if ((iTemp301) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp69)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp69)) });
            let mut iTemp302: i32 = (if ((iTemp288) != 0) { (if ((iTemp294) != 0) { (if ((((((iTemp298 == iTemp285)) as i32) | (iTemp299 & iTemp300))) != 0) { (((self.fConst1 * (if ((((iTemp301 & iTemp299) & iTemp300)) != 0) { (0.05000000074505806f32 * fTemp70) } else { fTemp70 }))) as i32) } else { 0i32 }) } else { 0i32 }) } else { (iTemp286).wrapping_sub((if ((iTemp287) != 0) { 1i32 } else { 0i32 })) });
            let mut iTemp303: i32 = (if ((iTemp0) != 0) { (((iTemp284 > iTemp285)) as i32) } else { (if ((iTemp2) != 0) { (((iTemp283 > 0i32)) as i32) } else { self.iRec17364_3[(1i32) as usize] }) });
            let mut iTemp304: i32 = (if ((iTemp288) != 0) { (if ((iTemp294) != 0) { (((iTemp298 > iTemp285)) as i32) } else { iTemp303 }) } else { iTemp303 });
            let mut iTemp305: i32 = ((((iTemp302 == 0i32)) as i32)).wrapping_mul(((((iTemp304 == 0i32)) as i32)).wrapping_add(1i32));
            let mut iTemp306: i32 = (((iTemp305 >= 2i32)) as i32);
            let mut iTemp307: i32 = (((iTemp305 >= 1i32)) as i32);
            let mut iTemp308: i32 = i32::max(112459776i32, iTemp285);
            let mut iTemp309: i32 = (if ((iTemp0) != 0) { iSlow176 } else { (if ((iTemp2) != 0) { iSlow174 } else { self.iRec17364_4[(1i32) as usize] }) });
            let mut iTemp310: i32 = i32::min((iSlow164).wrapping_add(((41i32).wrapping_mul(((fTemp68) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp311: i32 = (if ((iTemp288) != 0) { (if ((iTemp294) != 0) { (((self.fConst1 * (((((iTemp310 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp310).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp309 }) } else { iTemp309 });
            let mut iTemp312: i32 = (iTemp308).wrapping_add((((285212672i32).wrapping_sub(iTemp308)).wrapping_shr((24i32) as u32)).wrapping_mul(iTemp311));
            let mut iTemp313: i32 = (if ((iTemp0) != 0) { iTemp284 } else { (if ((iTemp2) != 0) { iTemp283 } else { self.iRec17364_2[(1i32) as usize] }) });
            let mut iTemp314: i32 = (if ((iTemp288) != 0) { (if ((iTemp294) != 0) { iTemp298 } else { iTemp313 }) } else { iTemp313 });
            let mut iTemp315: i32 = ((((((iTemp312 >= iTemp314)) as i32) >= 1i32)) as i32);
            let mut iTemp316: i32 = (iTemp291).wrapping_add(1i32);
            let mut iTemp317: i32 = (iTemp285).wrapping_sub(iTemp311);
            let mut iTemp318: i32 = ((((((iTemp317 <= iTemp314)) as i32) >= 1i32)) as i32);
            let mut iTemp319: i32 = ((((((iTemp316 < 4i32)) as i32) >= 1i32)) as i32);
            let mut iTemp320: i32 = (((iTemp316 >= 2i32)) as i32);
            let mut iTemp321: i32 = (((iTemp316 >= 1i32)) as i32);
            let mut iTemp322: i32 = (((iTemp316 >= 3i32)) as i32);
            let mut fTemp71: f32 = (if ((iTemp320) != 0) { (if ((iTemp322) != 0) { fSlow185 } else { fSlow31 }) } else { (if ((iTemp321) != 0) { fSlow30 } else { fSlow173 }) });
            let mut iTemp323: i32 = (((f32::max(((fTemp65 + (((((((if (fTemp71 >= 20.0f32) { (fTemp71 + 28.0f32) } else { ((iTbl59[(i32::min(((f32::round(fTemp71)) as i32), 19i32)) as usize]) as f32) })) as i32)).wrapping_shr((1i32) as u32)).wrapping_shl((6i32) as u32)) as f32)) - 4256.0f32), 16.0f32)) as i32)).wrapping_shl((16i32) as u32);
            let mut iTemp324: i32 = (if ((iTemp319) != 0) { iTemp323 } else { iTemp314 });
            let mut iTemp325: i32 = (if ((iTemp319) != 0) { (((iTemp323 > iTemp314)) as i32) } else { iTemp304 });
            let mut fTemp72: f32 = (if ((iTemp320) != 0) { (if ((iTemp322) != 0) { fSlow187 } else { fSlow34 }) } else { (if ((iTemp321) != 0) { fSlow33 } else { fSlow180 }) });
            let mut iTemp326: i32 = i32::min((iSlow164).wrapping_add(((41i32).wrapping_mul(((fTemp72) as i32))).wrapping_shr((6i32) as u32)), 63i32);
            let mut iTemp327: i32 = (if ((iTemp319) != 0) { (((self.fConst1 * (((((iTemp326 & 3i32)).wrapping_add(4i32)).wrapping_shl((((iTemp326).wrapping_shr((2i32) as u32)).wrapping_add(2i32)) as u32)) as f32))) as i32) } else { iTemp311 });
            let mut iTemp328: i32 = (((iTemp316 == 0i32)) as i32);
            let mut iTemp329: i32 = (((fTemp71 == 0.0f32)) as i32);
            let mut fTemp73: f32 = f32::min((fSlow182 + fTemp72), 99.0f32);
            let mut iTemp330: i32 = (((fTemp73 < 77.0f32)) as i32);
            let mut fTemp74: f32 = (if ((iTemp330) != 0) { ((iTbl382[(i32::max(i32::min(76i32, ((f32::round(fTemp73)) as i32)), 0i32)) as usize]) as f32) } else { (20.0f32 * (99.0f32 - fTemp73)) });
            let mut iTemp331: i32 = (if ((iTemp319) != 0) { (if ((((((iTemp323 == iTemp314)) as i32) | (iTemp328 & iTemp329))) != 0) { (((self.fConst1 * (if ((((iTemp330 & iTemp328) & iTemp329)) != 0) { (0.05000000074505806f32 * fTemp74) } else { fTemp74 }))) as i32) } else { 0i32 }) } else { iTemp302 });
            let mut iRecBody56: i32 = (if ((iTemp15) != 0) { (if ((iTemp28) != 0) { (if ((iTemp40) != 0) { iTemp36 } else { iTemp39 }) } else { (if ((iTemp29) != 0) { (if ((iTemp37) != 0) { iTemp36 } else { iTemp34 }) } else { iTemp7 }) }) } else { iTemp7 });
            let mut iRecBody57: i32 = (if ((iTemp15) != 0) { (if ((iTemp28) != 0) { (if ((iTemp40) != 0) { iTemp38 } else { iTemp13 }) } else { (if ((iTemp29) != 0) { (if ((iTemp37) != 0) { iTemp38 } else { iTemp13 }) } else { iTemp13 }) }) } else { iTemp13 });
            let mut iRecBody58: i32 = (if ((iTemp15) != 0) { (if ((iTemp28) != 0) { (if ((iTemp40) != 0) { iTemp46 } else { iTemp36 }) } else { (if ((iTemp29) != 0) { (if ((iTemp37) != 0) { iTemp46 } else { iTemp36 }) } else { iTemp36 }) }) } else { iTemp36 });
            let mut iRecBody59: i32 = (if ((iTemp15) != 0) { (if ((iTemp28) != 0) { (if ((iTemp40) != 0) { iTemp47 } else { iTemp26 }) } else { (if ((iTemp29) != 0) { (if ((iTemp37) != 0) { iTemp47 } else { iTemp26 }) } else { iTemp26 }) }) } else { iTemp26 });
            let mut iRecBody60: i32 = (if ((iTemp15) != 0) { (if ((iTemp28) != 0) { (if ((iTemp40) != 0) { iTemp49 } else { iTemp33 }) } else { (if ((iTemp29) != 0) { (if ((iTemp37) != 0) { iTemp49 } else { iTemp33 }) } else { iTemp33 }) }) } else { iTemp33 });
            let mut iRecBody61: i32 = iTemp14;
            let mut iRecBody62: i32 = (if ((iTemp15) != 0) { (if ((iTemp28) != 0) { (if ((iTemp40) != 0) { iTemp53 } else { iTemp24 }) } else { (if ((iTemp29) != 0) { (if ((iTemp37) != 0) { iTemp53 } else { iTemp24 }) } else { iTemp24 }) }) } else { iTemp24 });
            self.iRec15971[(0i32) as usize] = iRecBody56;
            self.iRec15971_1[(0i32) as usize] = iRecBody57;
            self.iRec15971_2[(0i32) as usize] = iRecBody58;
            self.iRec15971_3[(0i32) as usize] = iRecBody59;
            self.iRec15971_4[(0i32) as usize] = iRecBody60;
            self.iRec15971_5[(0i32) as usize] = iRecBody61;
            self.iRec15971_6[(0i32) as usize] = iRecBody62;
            let mut iTemp332: i32 = iTbl734[(iSlow6) as usize];
            let mut iTemp333: i32 = (((fTemp16 < 0.5f32)) as i32);
            let mut fTemp75: f32 = (1.0f32 - fTemp14);
            let mut fRecBody63: f32 = fTemp16;
            let mut fRecBody64: f32 = (if ((iTemp55) != 0) { fTemp13 } else { (if ((iTemp56) != 0) { fTemp13 } else { fTemp12 }) });
            let mut fRecBody65: f32 = fSlow79;
            let mut fRecBody66: f32 = fSlow78;
            let mut iRecBody67: i32 = (if ((iSlow8) != 0) { (if ((iSlow11) != 0) { iTemp58 } else { iTemp57 }) } else { iTemp57 });
            let mut fRecBody68: f32 = (if ((iSlow8) != 0) { (if ((iSlow11) != 0) { (0.003921568859368563f32 * ((iTemp58) as f32)) } else { (if ((iSlow12) != 0) { (0.5f32 * (f32::sin((6.2831854820251465f32 * fTemp16)) + 1.0f32)) } else { (if ((iTemp333) != 0) { 1.0f32 } else { 0.0f32 }) }) }) } else { (if ((iSlow9) != 0) { fTemp16 } else { (if ((iSlow10) != 0) { ((fTemp15 + fTemp75) - fSlow76) } else { (if ((iTemp333) != 0) { (2.0f32 * fTemp16) } else { (2.0f32 * fTemp75) }) }) }) });
            let mut fRecBody69: f32 = (if ((iTemp55) != 0) { (fTemp13 - 1.0f32) } else { (if ((iTemp56) != 0) { 0.0f32 } else { 1.0f32 }) });
            self.fRec16024[(0i32) as usize] = fRecBody63;
            self.fRec16024_1[(0i32) as usize] = fRecBody64;
            self.fRec16024_2[(0i32) as usize] = fRecBody65;
            self.fRec16024_3[(0i32) as usize] = fRecBody66;
            self.iRec16024_4[(0i32) as usize] = iRecBody67;
            self.fRec16024_5[(0i32) as usize] = fRecBody68;
            self.fRec16024_6[(0i32) as usize] = fRecBody69;
            let mut fTemp76: f32 = (1.0f32 - self.fRec16024_5[(0i32) as usize]);
            let mut iTemp334: i32 = (iSlow13 & iTemp1);
            let mut fRecBody70: f32 = (if ((iTemp61) != 0) { (if ((iTemp65) != 0) { (if ((iTemp69) != 0) { fTemp22 } else { fTemp23 }) } else { (if ((iTemp67) != 0) { fTemp22 } else { fTemp21 }) }) } else { fTemp19 });
            let mut iRecBody71: i32 = (if ((iTemp61) != 0) { (if ((iTemp65) != 0) { (if ((iTemp69) != 0) { iTemp68 } else { iTemp59 }) } else { (if ((iTemp67) != 0) { iTemp68 } else { iTemp59 }) }) } else { iTemp59 });
            let mut iRecBody72: i32 = (if ((iTemp61) != 0) { (if ((iTemp65) != 0) { (if ((iTemp69) != 0) { iTemp75 } else { iTemp66 }) } else { (if ((iTemp67) != 0) { iTemp75 } else { iTemp66 }) }) } else { iTemp66 });
            let mut iRecBody73: i32 = (if ((iTemp61) != 0) { (if ((iTemp65) != 0) { (if ((iTemp69) != 0) { iTemp76 } else { iTemp64 }) } else { (if ((iTemp67) != 0) { iTemp76 } else { iTemp64 }) }) } else { iTemp64 });
            let mut fRecBody74: f32 = (if ((iTemp61) != 0) { (if ((iTemp65) != 0) { (if ((iTemp69) != 0) { fTemp24 } else { fTemp20 }) } else { (if ((iTemp67) != 0) { fTemp24 } else { fTemp20 }) }) } else { fTemp20 });
            let mut iRecBody75: i32 = iTemp60;
            self.fRec16090[(0i32) as usize] = fRecBody70;
            self.iRec16090_1[(0i32) as usize] = iRecBody71;
            self.iRec16090_2[(0i32) as usize] = iRecBody72;
            self.iRec16090_3[(0i32) as usize] = iRecBody73;
            self.fRec16090_4[(0i32) as usize] = fRecBody74;
            self.iRec16090_5[(0i32) as usize] = iRecBody75;
            let mut fTemp77: f32 = ((iTbl1202[(iSlow18) as usize]) as f32);
            let mut fTemp78: f32 = (self.fRec16024_5[(0i32) as usize] - 0.5f32);
            let mut fTemp79: f32 = ((524288.0f32 * self.fRec16090[(0i32) as usize]) + (16777216.0f32 * (f32::abs((fSlow88 * ((fTemp77 * self.fRec16024_6[(0i32) as usize]) * fTemp78))) * (if ((0.00390625f32 * (fTemp77 * fTemp78)) < 0.0f32) { -1.0f32 } else { 1.0f32 }))));
            let mut fTemp80: f32 = (if ((iTemp334) != 0) { 0.0f32 } else { (self.fRec17426 + (self.fConst2 * f32::powf(2.0f32, (5.960464477539063e-8f32 * ((if ((iSlow14) != 0) { fSlow84 } else { (fSlow82 + (((iTbl938[(iSlow16) as usize]) as f32) + fSlow217)) }) + (if ((iSlow14) != 0) { 0.0f32 } else { fTemp79 })))))) });
            let mut fRecCur17426: f32 = (fTemp80 - f32::floor(fTemp80));
            let mut iRecBody76: i32 = (if ((iTemp89) != 0) { (if ((iTemp102) != 0) { (if ((iTemp114) != 0) { iTemp110 } else { iTemp113 }) } else { (if ((iTemp103) != 0) { (if ((iTemp111) != 0) { iTemp110 } else { iTemp108 }) } else { iTemp81 }) }) } else { iTemp81 });
            let mut iRecBody77: i32 = (if ((iTemp89) != 0) { (if ((iTemp102) != 0) { (if ((iTemp114) != 0) { iTemp112 } else { iTemp87 }) } else { (if ((iTemp103) != 0) { (if ((iTemp111) != 0) { iTemp112 } else { iTemp87 }) } else { iTemp87 }) }) } else { iTemp87 });
            let mut iRecBody78: i32 = (if ((iTemp89) != 0) { (if ((iTemp102) != 0) { (if ((iTemp114) != 0) { iTemp120 } else { iTemp110 }) } else { (if ((iTemp103) != 0) { (if ((iTemp111) != 0) { iTemp120 } else { iTemp110 }) } else { iTemp110 }) }) } else { iTemp110 });
            let mut iRecBody79: i32 = (if ((iTemp89) != 0) { (if ((iTemp102) != 0) { (if ((iTemp114) != 0) { iTemp121 } else { iTemp100 }) } else { (if ((iTemp103) != 0) { (if ((iTemp111) != 0) { iTemp121 } else { iTemp100 }) } else { iTemp100 }) }) } else { iTemp100 });
            let mut iRecBody80: i32 = (if ((iTemp89) != 0) { (if ((iTemp102) != 0) { (if ((iTemp114) != 0) { iTemp123 } else { iTemp107 }) } else { (if ((iTemp103) != 0) { (if ((iTemp111) != 0) { iTemp123 } else { iTemp107 }) } else { iTemp107 }) }) } else { iTemp107 });
            let mut iRecBody81: i32 = iTemp88;
            let mut iRecBody82: i32 = (if ((iTemp89) != 0) { (if ((iTemp102) != 0) { (if ((iTemp114) != 0) { iTemp127 } else { iTemp98 }) } else { (if ((iTemp103) != 0) { (if ((iTemp111) != 0) { iTemp127 } else { iTemp98 }) } else { iTemp98 }) }) } else { iTemp98 });
            self.iRec16339[(0i32) as usize] = iRecBody76;
            self.iRec16339_1[(0i32) as usize] = iRecBody77;
            self.iRec16339_2[(0i32) as usize] = iRecBody78;
            self.iRec16339_3[(0i32) as usize] = iRecBody79;
            self.iRec16339_4[(0i32) as usize] = iRecBody80;
            self.iRec16339_5[(0i32) as usize] = iRecBody81;
            self.iRec16339_6[(0i32) as usize] = iRecBody82;
            let mut iTemp335: i32 = iTbl734[(iSlow25) as usize];
            let mut fTemp81: f32 = (if ((iTemp334) != 0) { 0.0f32 } else { (self.fRec17443 + (self.fConst2 * f32::powf(2.0f32, (5.960464477539063e-8f32 * ((if ((iSlow26) != 0) { fSlow109 } else { (fSlow107 + (((iTbl938[(iSlow28) as usize]) as f32) + fSlow218)) }) + (if ((iSlow26) != 0) { 0.0f32 } else { fTemp79 })))))) });
            let mut fRecCur17443: f32 = (fTemp81 - f32::floor(fTemp81));
            let mut iRecBody83: i32 = (if ((iTemp140) != 0) { (if ((iTemp153) != 0) { (if ((iTemp165) != 0) { iTemp161 } else { iTemp164 }) } else { (if ((iTemp154) != 0) { (if ((iTemp162) != 0) { iTemp161 } else { iTemp159 }) } else { iTemp132 }) }) } else { iTemp132 });
            let mut iRecBody84: i32 = (if ((iTemp140) != 0) { (if ((iTemp153) != 0) { (if ((iTemp165) != 0) { iTemp163 } else { iTemp138 }) } else { (if ((iTemp154) != 0) { (if ((iTemp162) != 0) { iTemp163 } else { iTemp138 }) } else { iTemp138 }) }) } else { iTemp138 });
            let mut iRecBody85: i32 = (if ((iTemp140) != 0) { (if ((iTemp153) != 0) { (if ((iTemp165) != 0) { iTemp171 } else { iTemp161 }) } else { (if ((iTemp154) != 0) { (if ((iTemp162) != 0) { iTemp171 } else { iTemp161 }) } else { iTemp161 }) }) } else { iTemp161 });
            let mut iRecBody86: i32 = (if ((iTemp140) != 0) { (if ((iTemp153) != 0) { (if ((iTemp165) != 0) { iTemp172 } else { iTemp151 }) } else { (if ((iTemp154) != 0) { (if ((iTemp162) != 0) { iTemp172 } else { iTemp151 }) } else { iTemp151 }) }) } else { iTemp151 });
            let mut iRecBody87: i32 = (if ((iTemp140) != 0) { (if ((iTemp153) != 0) { (if ((iTemp165) != 0) { iTemp174 } else { iTemp158 }) } else { (if ((iTemp154) != 0) { (if ((iTemp162) != 0) { iTemp174 } else { iTemp158 }) } else { iTemp158 }) }) } else { iTemp158 });
            let mut iRecBody88: i32 = iTemp139;
            let mut iRecBody89: i32 = (if ((iTemp140) != 0) { (if ((iTemp153) != 0) { (if ((iTemp165) != 0) { iTemp178 } else { iTemp149 }) } else { (if ((iTemp154) != 0) { (if ((iTemp162) != 0) { iTemp178 } else { iTemp149 }) } else { iTemp149 }) }) } else { iTemp149 });
            self.iRec16599[(0i32) as usize] = iRecBody83;
            self.iRec16599_1[(0i32) as usize] = iRecBody84;
            self.iRec16599_2[(0i32) as usize] = iRecBody85;
            self.iRec16599_3[(0i32) as usize] = iRecBody86;
            self.iRec16599_4[(0i32) as usize] = iRecBody87;
            self.iRec16599_5[(0i32) as usize] = iRecBody88;
            self.iRec16599_6[(0i32) as usize] = iRecBody89;
            let mut iTemp336: i32 = iTbl734[(iSlow35) as usize];
            let mut fTemp82: f32 = (if ((iTemp334) != 0) { 0.0f32 } else { (self.fRec17468 + (self.fConst2 * f32::powf(2.0f32, (5.960464477539063e-8f32 * ((if ((iSlow36) != 0) { fSlow130 } else { (fSlow128 + (((iTbl938[(iSlow38) as usize]) as f32) + fSlow219)) }) + (if ((iSlow36) != 0) { 0.0f32 } else { fTemp79 })))))) });
            let mut fRecCur17468: f32 = (fTemp82 - f32::floor(fTemp82));
            let mut iRecBody90: i32 = (if ((iTemp191) != 0) { (if ((iTemp204) != 0) { (if ((iTemp216) != 0) { iTemp212 } else { iTemp215 }) } else { (if ((iTemp205) != 0) { (if ((iTemp213) != 0) { iTemp212 } else { iTemp210 }) } else { iTemp183 }) }) } else { iTemp183 });
            let mut iRecBody91: i32 = (if ((iTemp191) != 0) { (if ((iTemp204) != 0) { (if ((iTemp216) != 0) { iTemp214 } else { iTemp189 }) } else { (if ((iTemp205) != 0) { (if ((iTemp213) != 0) { iTemp214 } else { iTemp189 }) } else { iTemp189 }) }) } else { iTemp189 });
            let mut iRecBody92: i32 = (if ((iTemp191) != 0) { (if ((iTemp204) != 0) { (if ((iTemp216) != 0) { iTemp222 } else { iTemp212 }) } else { (if ((iTemp205) != 0) { (if ((iTemp213) != 0) { iTemp222 } else { iTemp212 }) } else { iTemp212 }) }) } else { iTemp212 });
            let mut iRecBody93: i32 = (if ((iTemp191) != 0) { (if ((iTemp204) != 0) { (if ((iTemp216) != 0) { iTemp223 } else { iTemp202 }) } else { (if ((iTemp205) != 0) { (if ((iTemp213) != 0) { iTemp223 } else { iTemp202 }) } else { iTemp202 }) }) } else { iTemp202 });
            let mut iRecBody94: i32 = (if ((iTemp191) != 0) { (if ((iTemp204) != 0) { (if ((iTemp216) != 0) { iTemp225 } else { iTemp209 }) } else { (if ((iTemp205) != 0) { (if ((iTemp213) != 0) { iTemp225 } else { iTemp209 }) } else { iTemp209 }) }) } else { iTemp209 });
            let mut iRecBody95: i32 = iTemp190;
            let mut iRecBody96: i32 = (if ((iTemp191) != 0) { (if ((iTemp204) != 0) { (if ((iTemp216) != 0) { iTemp229 } else { iTemp200 }) } else { (if ((iTemp205) != 0) { (if ((iTemp213) != 0) { iTemp229 } else { iTemp200 }) } else { iTemp200 }) }) } else { iTemp200 });
            self.iRec16851[(0i32) as usize] = iRecBody90;
            self.iRec16851_1[(0i32) as usize] = iRecBody91;
            self.iRec16851_2[(0i32) as usize] = iRecBody92;
            self.iRec16851_3[(0i32) as usize] = iRecBody93;
            self.iRec16851_4[(0i32) as usize] = iRecBody94;
            self.iRec16851_5[(0i32) as usize] = iRecBody95;
            self.iRec16851_6[(0i32) as usize] = iRecBody96;
            let mut iTemp337: i32 = iTbl734[(iSlow45) as usize];
            let mut fTemp83: f32 = (if ((iTemp334) != 0) { 0.0f32 } else { (self.fRec17485 + (self.fConst2 * f32::powf(2.0f32, (5.960464477539063e-8f32 * ((if ((iSlow46) != 0) { fSlow151 } else { (fSlow149 + (((iTbl938[(iSlow48) as usize]) as f32) + fSlow220)) }) + (if ((iSlow46) != 0) { 0.0f32 } else { fTemp79 })))))) });
            let mut fRecCur17485: f32 = (fTemp83 - f32::floor(fTemp83));
            let mut iRecBody97: i32 = (if ((iTemp242) != 0) { (if ((iTemp255) != 0) { (if ((iTemp267) != 0) { iTemp263 } else { iTemp266 }) } else { (if ((iTemp256) != 0) { (if ((iTemp264) != 0) { iTemp263 } else { iTemp261 }) } else { iTemp234 }) }) } else { iTemp234 });
            let mut iRecBody98: i32 = (if ((iTemp242) != 0) { (if ((iTemp255) != 0) { (if ((iTemp267) != 0) { iTemp265 } else { iTemp240 }) } else { (if ((iTemp256) != 0) { (if ((iTemp264) != 0) { iTemp265 } else { iTemp240 }) } else { iTemp240 }) }) } else { iTemp240 });
            let mut iRecBody99: i32 = (if ((iTemp242) != 0) { (if ((iTemp255) != 0) { (if ((iTemp267) != 0) { iTemp273 } else { iTemp263 }) } else { (if ((iTemp256) != 0) { (if ((iTemp264) != 0) { iTemp273 } else { iTemp263 }) } else { iTemp263 }) }) } else { iTemp263 });
            let mut iRecBody100: i32 = (if ((iTemp242) != 0) { (if ((iTemp255) != 0) { (if ((iTemp267) != 0) { iTemp274 } else { iTemp253 }) } else { (if ((iTemp256) != 0) { (if ((iTemp264) != 0) { iTemp274 } else { iTemp253 }) } else { iTemp253 }) }) } else { iTemp253 });
            let mut iRecBody101: i32 = (if ((iTemp242) != 0) { (if ((iTemp255) != 0) { (if ((iTemp267) != 0) { iTemp276 } else { iTemp260 }) } else { (if ((iTemp256) != 0) { (if ((iTemp264) != 0) { iTemp276 } else { iTemp260 }) } else { iTemp260 }) }) } else { iTemp260 });
            let mut iRecBody102: i32 = iTemp241;
            let mut iRecBody103: i32 = (if ((iTemp242) != 0) { (if ((iTemp255) != 0) { (if ((iTemp267) != 0) { iTemp280 } else { iTemp251 }) } else { (if ((iTemp256) != 0) { (if ((iTemp264) != 0) { iTemp280 } else { iTemp251 }) } else { iTemp251 }) }) } else { iTemp251 });
            self.iRec17112[(0i32) as usize] = iRecBody97;
            self.iRec17112_1[(0i32) as usize] = iRecBody98;
            self.iRec17112_2[(0i32) as usize] = iRecBody99;
            self.iRec17112_3[(0i32) as usize] = iRecBody100;
            self.iRec17112_4[(0i32) as usize] = iRecBody101;
            self.iRec17112_5[(0i32) as usize] = iRecBody102;
            self.iRec17112_6[(0i32) as usize] = iRecBody103;
            let mut iTemp338: i32 = iTbl734[(iSlow55) as usize];
            let mut fTemp84: f32 = (if ((iTemp334) != 0) { 0.0f32 } else { (self.fRec17511 + (self.fConst2 * f32::powf(2.0f32, (5.960464477539063e-8f32 * ((if ((iSlow56) != 0) { fSlow172 } else { (fSlow170 + (((iTbl938[(iSlow58) as usize]) as f32) + fSlow221)) }) + (if ((iSlow56) != 0) { 0.0f32 } else { fTemp79 })))))) });
            let mut fRecCur17511: f32 = (fTemp84 - f32::floor(fTemp84));
            let mut iRecBody104: i32 = (if ((iTemp293) != 0) { (if ((iTemp306) != 0) { (if ((iTemp318) != 0) { iTemp314 } else { iTemp317 }) } else { (if ((iTemp307) != 0) { (if ((iTemp315) != 0) { iTemp314 } else { iTemp312 }) } else { iTemp285 }) }) } else { iTemp285 });
            let mut iRecBody105: i32 = (if ((iTemp293) != 0) { (if ((iTemp306) != 0) { (if ((iTemp318) != 0) { iTemp316 } else { iTemp291 }) } else { (if ((iTemp307) != 0) { (if ((iTemp315) != 0) { iTemp316 } else { iTemp291 }) } else { iTemp291 }) }) } else { iTemp291 });
            let mut iRecBody106: i32 = (if ((iTemp293) != 0) { (if ((iTemp306) != 0) { (if ((iTemp318) != 0) { iTemp324 } else { iTemp314 }) } else { (if ((iTemp307) != 0) { (if ((iTemp315) != 0) { iTemp324 } else { iTemp314 }) } else { iTemp314 }) }) } else { iTemp314 });
            let mut iRecBody107: i32 = (if ((iTemp293) != 0) { (if ((iTemp306) != 0) { (if ((iTemp318) != 0) { iTemp325 } else { iTemp304 }) } else { (if ((iTemp307) != 0) { (if ((iTemp315) != 0) { iTemp325 } else { iTemp304 }) } else { iTemp304 }) }) } else { iTemp304 });
            let mut iRecBody108: i32 = (if ((iTemp293) != 0) { (if ((iTemp306) != 0) { (if ((iTemp318) != 0) { iTemp327 } else { iTemp311 }) } else { (if ((iTemp307) != 0) { (if ((iTemp315) != 0) { iTemp327 } else { iTemp311 }) } else { iTemp311 }) }) } else { iTemp311 });
            let mut iRecBody109: i32 = iTemp292;
            let mut iRecBody110: i32 = (if ((iTemp293) != 0) { (if ((iTemp306) != 0) { (if ((iTemp318) != 0) { iTemp331 } else { iTemp302 }) } else { (if ((iTemp307) != 0) { (if ((iTemp315) != 0) { iTemp331 } else { iTemp302 }) } else { iTemp302 }) }) } else { iTemp302 });
            self.iRec17364[(0i32) as usize] = iRecBody104;
            self.iRec17364_1[(0i32) as usize] = iRecBody105;
            self.iRec17364_2[(0i32) as usize] = iRecBody106;
            self.iRec17364_3[(0i32) as usize] = iRecBody107;
            self.iRec17364_4[(0i32) as usize] = iRecBody108;
            self.iRec17364_5[(0i32) as usize] = iRecBody109;
            self.iRec17364_6[(0i32) as usize] = iRecBody110;
            let mut iTemp339: i32 = iTbl734[(iSlow65) as usize];
            let mut fTemp85: f32 = (if ((iTemp334) != 0) { 0.0f32 } else { (self.fRec17528 + (self.fConst2 * f32::powf(2.0f32, (5.960464477539063e-8f32 * ((if ((iSlow66) != 0) { fSlow193 } else { (fSlow191 + (((iTbl938[(iSlow68) as usize]) as f32) + fSlow222)) }) + (if ((iSlow66) != 0) { 0.0f32 } else { fTemp79 })))))) });
            let mut fRecCur17528: f32 = (fTemp85 - f32::floor(fTemp85));
            let mut fRecCur17536: f32 = (0.5f32 * (f32::powf(2.0f32, (5.960464477539063e-8f32 * (((self.iRec17364[(0i32) as usize]).wrapping_sub(((if (iTemp339 != 0i32) { ((((5.960464477539063e-8f32 * (((self.iRec17364[(0i32) as usize]) as f32) * f32::exp(((fSlow80 * ((((iTemp339) as f32) * self.fRec16024_6[(0i32) as usize]) * fTemp76)) + 12.199999809265137f32)))) + 0.5f32)) as i32) } else { 0i32 })).wrapping_add(234881024i32))) as f32))) * f32::sin((6.2831854820251465f32 * (fRecCur17528 + (fSlow195 * self.fRec17536))))));
            let mut fTemp86: f32 = (0.5f32 * (((f32::powf(2.0f32, (5.960464477539063e-8f32 * (((self.iRec15971[(0i32) as usize]).wrapping_sub(((if (iTemp332 != 0i32) { ((((5.960464477539063e-8f32 * (((self.iRec15971[(0i32) as usize]) as f32) * f32::exp(((fSlow80 * ((((iTemp332) as f32) * self.fRec16024_6[(0i32) as usize]) * fTemp76)) + 12.199999809265137f32)))) + 0.5f32)) as i32) } else { 0i32 })).wrapping_add(234881024i32))) as f32))) * f32::sin((6.2831854820251465f32 * (fRecCur17426 + (0.5f32 * (f32::powf(2.0f32, (5.960464477539063e-8f32 * (((self.iRec16339[(0i32) as usize]).wrapping_sub(((if (iTemp335 != 0i32) { ((((5.960464477539063e-8f32 * (((self.iRec16339[(0i32) as usize]) as f32) * f32::exp(((fSlow80 * ((((iTemp335) as f32) * self.fRec16024_6[(0i32) as usize]) * fTemp76)) + 12.199999809265137f32)))) + 0.5f32)) as i32) } else { 0i32 })).wrapping_add(234881024i32))) as f32))) * f32::sin((6.2831854820251465f32 * fRecCur17443)))))))) + (f32::powf(2.0f32, (5.960464477539063e-8f32 * (((self.iRec16599[(0i32) as usize]).wrapping_sub(((if (iTemp336 != 0i32) { ((((5.960464477539063e-8f32 * (((self.iRec16599[(0i32) as usize]) as f32) * f32::exp(((fSlow80 * ((((iTemp336) as f32) * self.fRec16024_6[(0i32) as usize]) * fTemp76)) + 12.199999809265137f32)))) + 0.5f32)) as i32) } else { 0i32 })).wrapping_add(234881024i32))) as f32))) * f32::sin((6.2831854820251465f32 * (fRecCur17468 + (0.5f32 * (f32::powf(2.0f32, (5.960464477539063e-8f32 * (((self.iRec16851[(0i32) as usize]).wrapping_sub(((if (iTemp337 != 0i32) { ((((5.960464477539063e-8f32 * (((self.iRec16851[(0i32) as usize]) as f32) * f32::exp(((fSlow80 * ((((iTemp337) as f32) * self.fRec16024_6[(0i32) as usize]) * fTemp76)) + 12.199999809265137f32)))) + 0.5f32)) as i32) } else { 0i32 })).wrapping_add(234881024i32))) as f32))) * f32::sin((6.2831854820251465f32 * fRecCur17485))))))))) + (f32::powf(2.0f32, (5.960464477539063e-8f32 * (((self.iRec17112[(0i32) as usize]).wrapping_sub(((if (iTemp338 != 0i32) { ((((5.960464477539063e-8f32 * (((self.iRec17112[(0i32) as usize]) as f32) * f32::exp(((fSlow80 * ((((iTemp338) as f32) * self.fRec16024_6[(0i32) as usize]) * fTemp76)) + 12.199999809265137f32)))) + 0.5f32)) as i32) } else { 0i32 })).wrapping_add(234881024i32))) as f32))) * f32::sin((6.2831854820251465f32 * (fRecCur17511 + fRecCur17536))))));
            let mut fTemp87: FaustFloat = ((fTemp86) as FaustFloat);
            output0[(i0) as usize] = fTemp87;
            output1[(i0) as usize] = fTemp87;
            self.fVec12[(1i32) as usize] = self.fVec12[(0i32) as usize];
            self.iRec15971[(1i32) as usize] = self.iRec15971[(0i32) as usize];
            self.iRec15971_1[(1i32) as usize] = self.iRec15971_1[(0i32) as usize];
            self.iRec15971_2[(1i32) as usize] = self.iRec15971_2[(0i32) as usize];
            self.iRec15971_3[(1i32) as usize] = self.iRec15971_3[(0i32) as usize];
            self.iRec15971_4[(1i32) as usize] = self.iRec15971_4[(0i32) as usize];
            self.iRec15971_5[(1i32) as usize] = self.iRec15971_5[(0i32) as usize];
            self.iRec15971_6[(1i32) as usize] = self.iRec15971_6[(0i32) as usize];
            self.fRec16024[(1i32) as usize] = self.fRec16024[(0i32) as usize];
            self.fRec16024_1[(1i32) as usize] = self.fRec16024_1[(0i32) as usize];
            self.fRec16024_2[(1i32) as usize] = self.fRec16024_2[(0i32) as usize];
            self.fRec16024_3[(1i32) as usize] = self.fRec16024_3[(0i32) as usize];
            self.iRec16024_4[(1i32) as usize] = self.iRec16024_4[(0i32) as usize];
            self.fRec16024_5[(1i32) as usize] = self.fRec16024_5[(0i32) as usize];
            self.fRec16024_6[(1i32) as usize] = self.fRec16024_6[(0i32) as usize];
            self.fRec16090[(1i32) as usize] = self.fRec16090[(0i32) as usize];
            self.iRec16090_1[(1i32) as usize] = self.iRec16090_1[(0i32) as usize];
            self.iRec16090_2[(1i32) as usize] = self.iRec16090_2[(0i32) as usize];
            self.iRec16090_3[(1i32) as usize] = self.iRec16090_3[(0i32) as usize];
            self.fRec16090_4[(1i32) as usize] = self.fRec16090_4[(0i32) as usize];
            self.iRec16090_5[(1i32) as usize] = self.iRec16090_5[(0i32) as usize];
            self.fRec17426 = fRecCur17426;
            self.iRec16339[(1i32) as usize] = self.iRec16339[(0i32) as usize];
            self.iRec16339_1[(1i32) as usize] = self.iRec16339_1[(0i32) as usize];
            self.iRec16339_2[(1i32) as usize] = self.iRec16339_2[(0i32) as usize];
            self.iRec16339_3[(1i32) as usize] = self.iRec16339_3[(0i32) as usize];
            self.iRec16339_4[(1i32) as usize] = self.iRec16339_4[(0i32) as usize];
            self.iRec16339_5[(1i32) as usize] = self.iRec16339_5[(0i32) as usize];
            self.iRec16339_6[(1i32) as usize] = self.iRec16339_6[(0i32) as usize];
            self.fRec17443 = fRecCur17443;
            self.iRec16599[(1i32) as usize] = self.iRec16599[(0i32) as usize];
            self.iRec16599_1[(1i32) as usize] = self.iRec16599_1[(0i32) as usize];
            self.iRec16599_2[(1i32) as usize] = self.iRec16599_2[(0i32) as usize];
            self.iRec16599_3[(1i32) as usize] = self.iRec16599_3[(0i32) as usize];
            self.iRec16599_4[(1i32) as usize] = self.iRec16599_4[(0i32) as usize];
            self.iRec16599_5[(1i32) as usize] = self.iRec16599_5[(0i32) as usize];
            self.iRec16599_6[(1i32) as usize] = self.iRec16599_6[(0i32) as usize];
            self.fRec17468 = fRecCur17468;
            self.iRec16851[(1i32) as usize] = self.iRec16851[(0i32) as usize];
            self.iRec16851_1[(1i32) as usize] = self.iRec16851_1[(0i32) as usize];
            self.iRec16851_2[(1i32) as usize] = self.iRec16851_2[(0i32) as usize];
            self.iRec16851_3[(1i32) as usize] = self.iRec16851_3[(0i32) as usize];
            self.iRec16851_4[(1i32) as usize] = self.iRec16851_4[(0i32) as usize];
            self.iRec16851_5[(1i32) as usize] = self.iRec16851_5[(0i32) as usize];
            self.iRec16851_6[(1i32) as usize] = self.iRec16851_6[(0i32) as usize];
            self.fRec17485 = fRecCur17485;
            self.iRec17112[(1i32) as usize] = self.iRec17112[(0i32) as usize];
            self.iRec17112_1[(1i32) as usize] = self.iRec17112_1[(0i32) as usize];
            self.iRec17112_2[(1i32) as usize] = self.iRec17112_2[(0i32) as usize];
            self.iRec17112_3[(1i32) as usize] = self.iRec17112_3[(0i32) as usize];
            self.iRec17112_4[(1i32) as usize] = self.iRec17112_4[(0i32) as usize];
            self.iRec17112_5[(1i32) as usize] = self.iRec17112_5[(0i32) as usize];
            self.iRec17112_6[(1i32) as usize] = self.iRec17112_6[(0i32) as usize];
            self.fRec17511 = fRecCur17511;
            self.iRec17364[(1i32) as usize] = self.iRec17364[(0i32) as usize];
            self.iRec17364_1[(1i32) as usize] = self.iRec17364_1[(0i32) as usize];
            self.iRec17364_2[(1i32) as usize] = self.iRec17364_2[(0i32) as usize];
            self.iRec17364_3[(1i32) as usize] = self.iRec17364_3[(0i32) as usize];
            self.iRec17364_4[(1i32) as usize] = self.iRec17364_4[(0i32) as usize];
            self.iRec17364_5[(1i32) as usize] = self.iRec17364_5[(0i32) as usize];
            self.iRec17364_6[(1i32) as usize] = self.iRec17364_6[(0i32) as usize];
            self.fRec17528 = fRecCur17528;
            self.fRec17536 = fRecCur17536;
        }
    }

}
impl FaustDsp for Dx7Piano {
    type T = FaustFloat;
    fn new() -> Self where Self: Sized { Self::new() }
    fn metadata(&self, m: &mut dyn Meta) { self.metadata(m) }
    fn get_sample_rate(&self) -> i32 { self.get_sample_rate() }
    fn get_num_inputs(&self) -> i32 { self.get_num_inputs() }
    fn get_num_outputs(&self) -> i32 { self.get_num_outputs() }
    fn class_init(sample_rate: i32) where Self: Sized { Self::class_init(sample_rate); }
    fn instance_reset_params(&mut self) { self.instance_reset_params() }
    fn instance_clear(&mut self) { self.instance_clear() }
    fn instance_constants(&mut self, sample_rate: i32) { self.instance_constants(sample_rate) }
    fn instance_init(&mut self, sample_rate: i32) { self.instance_init(sample_rate) }
    fn init(&mut self, sample_rate: i32) { self.init(sample_rate) }
    fn build_user_interface(&self, ui: &mut dyn UI<Self::T>) { self.build_user_interface(ui) }
    fn build_user_interface_static(ui: &mut dyn UI<Self::T>) where Self: Sized { Self::build_user_interface_static(ui); }
    fn get_param(&self, param: ParamIndex) -> Option<Self::T> { self.get_param(param) }
    fn set_param(&mut self, param: ParamIndex, value: Self::T) { self.set_param(param, value) }
    fn compute(&mut self, count: i32, inputs: &[&[Self::T]], outputs: &mut [&mut [Self::T]]) { self.compute(count as usize, inputs, outputs) }
}
