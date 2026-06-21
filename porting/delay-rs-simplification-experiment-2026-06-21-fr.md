# Expérience : rendre `signal_fir/delay.rs` plus simple à lire sans perdre en rigueur ni en vitesse

**Date :** 2026-06-21
**Périmètre :** `crates/transform/src/signal_fir/delay.rs` (1671 lignes), ses sites d'appel dans
`crates/transform/src/signal_fir/module/` et `recursion.rs`.
**Statut :** Analyse / conception — aucun code modifié pour l'instant.
**Objectif :** restructurer le sous-système de délais en étapes qu'un humain peut lire
indépendamment, documenter isolément et recombiner — tout en émettant un **FIR identique au
bit près** (donc un C/WASM généré identique et des performances d'exécution identiques).
**Documents compagnons :** [`delay-manager-design-2026-04-06-en.md`](delay-manager-design-2026-04-06-en.md),
[`delay-strategy-abstraction-plan-2026-04-08-en.md`](delay-strategy-abstraction-plan-2026-04-08-en.md),
[`delay-merging-plan-2026-04-05-en.md`](delay-merging-plan-2026-04-05-en.md),
[`cpp-delay-analysis-parity-plan-2026-04-08-en.md`](cpp-delay-analysis-parity-plan-2026-04-08-en.md),
[`signal-to-fir-transform-analysis-2026-06-20-en.md`](signal-to-fir-transform-analysis-2026-06-20-en.md).
**Jumeau anglais :** [`delay-rs-simplification-experiment-2026-06-21-en.md`](delay-rs-simplification-experiment-2026-06-21-en.md).

---

## 0. Où ce code se situe dans la chaîne de compilation

### 0.1 Le pipeline au-dessus de `delay.rs`

```
boxes ──► propagate ──► signals (+ UiProgram)
                            │
                            ▼
        ┌───────────────────────────────────────────────┐
        │ crates/transform  (abaissement de niveau)      │
        │                                                │
        │   signal_prepare ──► signal_fir ──► FIR        │
        │     (mise en scène)  (abaissement)             │
        └───────────────────────────────────────────────┘
                            │
                            ▼
              fir ──► codegen (C / C++ / WASM / Cranelift / FBC)
```

`transform` est la couche entre la *propagation* (qui possède le modèle de signaux et la récursion
en de-Bruijn) et les *backends FIR* (qui possèdent la génération de code). Le point d'entrée public
unique est `compile_signals_to_fir_fastlane_with_ui(...)` à
[`signal_fir/mod.rs:205`](../crates/transform/src/signal_fir/mod.rs), qui exécute trois étapes :
le contrôle de contrat (`planner`), la mise en scène (`signal_prepare`) et l'émission FIR
(`module::build_module`).

`delay.rs` est la partie de l'étape 3 qui transforme l'opérateur `@(n)` de Faust et les arêtes
d'état à un échantillon (`Delay1`, `Prefix`) en tampons circulaires, compteurs et instructions de
lecture/écriture concrets.

### 0.2 Ce dont le fichier est responsable

Le `@` de Faust correspond à l'une des trois stratégies de tampon, choisie selon le délai par
rapport à deux seuils (`-mcd` défaut 16, `-dlt` défaut `u32::MAX`) :

| Plage de délai | Stratégie | Tampon | Pointeur |
|----------------|-----------|--------|----------|
| `[1, mcd)` | **Shift** | exact `N+1` | aucun — décalage à chaque échantillon, lecture `buf[N]` |
| `[mcd, dlt)` | **CircularPow2** | `next_pow2(N+1)` | `fIOTA` partagé, index masqué |
| `[dlt, ∞)` | **IfWrapping** | exact `N+1` | `fIdx<id>` par ligne, repli par `if` |

Il gère aussi la **fusion récursion+délai** : lorsqu'un signal retardé lit en définitive depuis un
porteur de récursion actif (`Delay1^k(Proj(i, group))`), aucun tampon séparé n'est alloué ; le
tableau de récursion est agrandi pour contenir l'historique à la place.

### 0.3 Les trois phases auxquelles delay.rs participe

Le sous-système n'est **pas** un simple appel de fonction. Il est tissé dans trois moments distincts
d'une construction de module, avec un état (`DelayManager`) porté entre eux :

```
PHASE 1 — PRÉPARATION  (setup.rs::prepare_delay_lines, build.rs:152)
  delay.analyze_signals(...)   → remplit rec_output_analysis  (métadonnées de taille récursion)
  delay.scan_signals(...)      → renvoie max_delays: HashMap<SigId,i32>
  pour (carried, delay): ensure_delay_line(carried, delay, &mut DelayFirCtx)
                               → déclare fVec*/iVec*, enregistre la boucle instanceClear,
                                 déclare fIOTA ou fIdx* au besoin

PHASE 2 — ABAISSEMENT  (core_lowering.rs::lower_fixed_delay / lower_shift_delay1)
  module.rs garde l'orchestration (réutilisation récursion, éval. du montant, dédup. d'écriture),
  délègue la lecture/écriture concrète à :
      emit_fixed_delay_for_line(&mut DelayLoweringCtx, &line, ...)
      emit_delay1_for_line(&mut DelayLoweringCtx, &line, ...)

PHASE 3 — FIN D'ÉCHANTILLON  (build.rs:212 / build.rs:235)
  delay.emit_sample_end_updates(store, uses_iota)
                               → fIOTA += 1, et l'avance avec repli de chaque fIdx*

TRANSVERSAL — RÉCURSION  (recursion.rs::ensure_recursion_array_for_group)
  lit delay.rec_output_analysis(var, index) pour dimensionner les tableaux de récursion qui
  servent aussi de tampons de délai fusionnés
```

Les données passées entre phases sont le nœud du problème : `DelayManager` possède `delay_lines`,
`rec_output_analysis` et `scheduled_delay_writes` ; les deux faisceaux d'emprunt `DelayFirCtx`
(8 champs, au moment de l'allocation) et `DelayLoweringCtx` (4 champs, au moment de l'abaissement)
portent des références vers des champs disjoints de `SignalToFirLower`, de sorte que le manager et
le reste de l'abaisseur puissent être empruntés simultanément.

---

## 1. Ce que `delay.rs` contient aujourd'hui

Le fichier est correct, bien commenté, et déjà factorisé une fois (voir les deux docs de conception
d'avril 2026). Mais il empaquette **huit grappes de préoccupations distinctes** dans un seul module
de 1671 lignes :

| # | Grappe | Lignes | Nature |
|---|--------|--------|--------|
| 1 | Fns libres de dimensionnement/analyse (`pow2limit_for_delay`, `*_delay_amount`, `*_max_bound`, `delay_size_for_amount`) | 220–368 | pure, sans état |
| 2 | Types de données (`DelayOptions`, `DelayStrategy`, `DelayLineInfo`, `DelayAnalysisEntry`) | 129–218 | données simples |
| 3 | `GlobalCircularCursor` (le service `fIOTA`) | 370–439 | ZST émetteur |
| 4 | Trait `RingDelayModel` + `CircularPow2Model` + `IfWrappingModel` | 441–615 | géométrie |
| 5 | `DelayFirCtx` (faisceau d'emprunt d'allocation + ses méthodes) | 617–776 | câblage |
| 6 | `DelayLoweringCtx` + `DelayStrategyEmitter` + 2 émetteurs + dispatch | 778–983 | émission abaissement |
| 7 | Aides d'émission FIR libres (`masked_delay_index`, `emit_*shift*`, `if_wrapping_*`, `bump_*`) | 985–1145 | émission feuille |
| 8 | `DelayManager` (état + 2 parcours d'arbre + sélection + allocation + accesseurs) | 1147–1671 | orchestration |

Un nouveau venu doit tenir les huit en tête à la fois, car les préoccupations sont entrelacées par
*phase* plutôt que séparées par *concept* : par exemple « tout ce qui concerne `IfWrapping` » est
réparti entre les grappes 2 (variante d'enum), 4 (`IfWrappingModel`), 6 (bras de dispatch), 7
(`if_wrapping_read_index`, `bump_if_wrapping_counter`) et 8 (branche de sélection dans
`ensure_delay_line`).

---

## 2. Pourquoi c'est difficile à lire aujourd'hui (les frictions concrètes)

Ce sont les coûts précis que l'expérience doit éliminer. Chacun motive une ou plusieurs propositions
au §3.

**F1 — Deux parcours d'arbre quasi dupliqués.** `analyze_signals → analyze_node → analyze_child`
([`delay.rs:1222-1381`](../crates/transform/src/signal_fir/delay.rs)) et
`scan_signals → scan_node → scan_child` ([`delay.rs:1252-1491`](../crates/transform/src/signal_fir/delay.rs))
parcourent tous deux le DAG préparé, appellent tous deux `delay_size_for_amount`, traitent tous deux
spécialement `Delay`/`Delay1`/`Proj`, parcourent les enfants de liste avec le même passe-partout
`is_list/hd/tl`. Ils ne diffèrent que par *ce qu'ils accumulent* : `analyze` suit le délai accumulé
le long du chemin (mémoïsé par `best_seen_delay`, clé = valeur accumulée) pour dimensionner les
porteurs de récursion ; `scan` enregistre le délai possédé maximal par porteur (mémoïsé par un
ensemble `seen`) pour les lignes autonomes. Un lecteur doit comparer deux parcours d'environ 70
lignes pour voir qu'ils sont « le même parcours, deux accumulateurs ». Un *troisième* parcours dans
`recursion.rs` consomme ensuite la sortie du premier.

**F2 — Un seul concept de stratégie, cinq sites éparpillés.** `DelayStrategy` (données) /
`RingDelayModel` (géométrie, stratégies en anneau uniquement) / `DelayStrategyEmitter` (abaissement
complet, les trois) sont trois abstractions pour une seule idée. Le dispatch à 3 branches est écrit
deux fois (`emit_fixed_delay_for_line` et `emit_delay1_for_line`,
[`delay.rs:937-978`](../crates/transform/src/signal_fir/delay.rs)). `runtime_state_for_line`
([`delay.rs:925`](../crates/transform/src/signal_fir/delay.rs)) mappe stratégie→état d'exécution avec
un `debug_assert!(false)` pour le cas `Shift` qui ne peut pas survenir.

**F3 — Des branches impossibles que les types n'interdisent pas.** Parce que `DelayRuntimeState` est
partagé par les deux modèles en anneau, `CircularPow2Model::write_index/read_index` portent des bras
`Counter(_)` jamais atteints (CircularPow2 est toujours `GlobalIota`), et
`IfWrappingModel::read_index/emit_advance` portent des replis `debug_assert!(false)` pour le cas
`GlobalIota` qui ne peut survenir ([`delay.rs:518-614`](../crates/transform/src/signal_fir/delay.rs)).
Ces bras morts n'existent que parce que l'invariant « le modèle M ne voit jamais que son état S(M) »
vit dans des commentaires, pas dans le type.

**F4 — Logique de sélection éclatée en trois dans une seule fonction.** `ensure_delay_line`
([`delay.rs:1533-1618`](../crates/transform/src/signal_fir/delay.rs)) choisit la stratégie dans un
`if/else`, calcule la taille dans un *deuxième* `match` sur la même stratégie, et émet les
déclarations annexes (`ensure_iota` / `ensure_if_wrapping_counter`) dans un *troisième* `match` —
trois `match` sur la même valeur, chacun un endroit où oublier un cas.

**F5 — Deux faisceaux d'emprunt, assemblés à la main à chaque site.** `DelayFirCtx` (8 champs) et
`DelayLoweringCtx` (4 champs) sont reconstruits avec la même incantation littéral-de-struct-avec-
emprunt-fractionné à quatre sites de `state.rs`/`core_lowering.rs`, chacun répétant la mise en garde
« ne PAS construire via `&mut self` ». Le couplage à la disposition des champs de
`SignalToFirLower` est implicite et facile à casser.

**F6 — Le passe-partout du builder enterre l'arithmétique.** Chaque constante et chaque opération
binaire est `let x = { let mut b = FirBuilder::new(store); b... };`. `if_wrapping_read_index` et
`bump_if_wrapping_counter` ([`delay.rs:1068-1145`](../crates/transform/src/signal_fir/delay.rs)) font
environ 40 lignes qui encodent deux formules d'une ligne — `(counter + size − amount) repli-si ≥ size`
et `(counter + 1 ≥ size) ? 0 : counter + 1` — que le commentaire d'en-tête du module énonce déjà en
ASCII mais que le code ne reflète pas visiblement.

Aucune de ces frictions n'est un bug. Ce sont toutes des taxes de compréhension payées à chaque
lecture.

---

## 3. Trois manières indépendantes de restructurer

Les trois propositions attaquent **trois axes orthogonaux** et peuvent être adoptées une à la fois
ou empilées :

- **A — vertical** — découper par *stratégie* : une unité autonome par stratégie de délai.
- **B — horizontal** — découper par *phase* : une passe d'analyse produisant une valeur explicite
  `DelayPlan`, puis un émetteur pur qui la consomme.
- **C — profondeur** — découper à la *feuille* : une fine couche arithmétique pour que les formules
  d'index se lisent comme les commentaires.

```
        profondeur (C : formules lisibles)
          ▲
          │
   ───────┼───────────────►  phase (B : plan → émission)
          │
          ▼
     stratégie (A : une unité par stratégie)
```

Chaque section donne l'idée, un croquis avant/après, ce qui devient plus simple, la surface
d'interaction (« composabilité »), le coût, et le degré d'indépendance vis-à-vis des deux autres.

### Proposition A — La stratégie comme objet fermé (découpe verticale)

**Idée.** Remplacer les fragments par-stratégie des grappes 2/4/6/7 par **un type cohésif par
stratégie** derrière un trait unique, de sorte qu'un lecteur intéressé par `IfWrapping` ouvre un seul
fichier et le lise de haut en bas.

```rust
/// Tout ce qu'une stratégie de délai doit répondre. Pas d'enum d'état partagé.
pub(super) trait DelayKind {
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError>;
    fn declare_state(&self, ctx: &mut DelayDecls);          // fIOTA / fIdx* / rien
    fn emit_read (&self, e: &mut Emit, line: &DelayLineInfo, amount: FirId, ty: FirType) -> FirId;
    fn emit_write(&self, e: &mut Emit, line: &DelayLineInfo, current: FirId);
    fn emit_advance(&self, e: &mut Emit, line: &DelayLineInfo) -> Option<FirId>;
}
```

`delay/` devient un petit répertoire :

```
signal_fir/delay/
  mod.rs            // ré-exports, la fn de sélection, DelayManager
  options.rs        // DelayOptions, sélecteur DelayStrategy
  shift.rs          // ShiftKind: taille, pas d'état, store@0 + boucle de décalage, lecture buf[N]
  circular_pow2.rs  // CircularPow2Kind: taille pow2, fIOTA, lecture/écriture/avance masquées
  if_wrapping.rs    // IfWrappingKind: taille exacte, fIdx*, lecture/avance par if-repli
  sizing.rs         // les fns libres pures de la grappe 1 (inchangées)
```

La sélection est une seule fonction renvoyant la stratégie choisie ; `DelayLineInfo` la stocke
(comme un `enum` pour un dispatch à coût nul — voir migration §4). L'enum partagé
`DelayRuntimeState`, `runtime_state_for_line`, le dispatch dupliqué `emit_*_for_line`, et les bras
impossibles `Counter(_)`/`GlobalIota` (F3) disparaissent tous, car chaque stratégie ne touche jamais
que son propre compteur.

**Ce qui devient plus simple.**
- *À lire :* un concept = un fichier ; les trois blocs ASCII des commentaires se trouvent désormais à
  côté du code qui les réalise.
- *À documenter :* chaque fichier a un en-tête `//!` ; plus de sauts « voir aussi » entre cinq sites
  (F2).
- *À étendre :* une quatrième stratégie est un quatrième fichier + un bras de sélecteur, rien
  d'autre.
- F3 et le double dispatch (F2) disparaissent par construction.

**Composabilité / surface d'interaction.** Exactement un trait à cinq méthodes. Les stratégies ne se
référencent jamais ; leur seul contrat est `DelayKind`. L'interaction manager↔stratégie est « le
sélecteur choisit une stratégie ; les phases appellent ses méthodes ».

**Coût.** Un trait + des types par stratégie ; il faut conserver un dispatch à coût nul (utiliser un
`enum DelayKind { Shift, CircularPow2, IfWrapping }` avec une cale `match`, *pas* `dyn`, pour
préserver la monomorphisation — voir performance §4).

**Indépendance.** Totalement indépendante de B et C. Touche l'organisation des grappes 2/4/6/7 ;
laisse les parcours d'arbre (grappe 8) et la math feuille (corps de la grappe 7) tels quels.

### Proposition B — Un pipeline `plan → émission` avec un `DelayPlan` explicite (découpe horizontale)

**Idée.** Fusionner les deux parcours d'arbre (F1) en **un seul parcours** dont la sortie est une
valeur inspectable et sans effet de bord — `DelayPlan` — et faire de chaque phase ultérieure un
lecteur pur de cette valeur.

```rust
/// Toute la décision de délai, en données simples. Pas de FIR, pas de FirStore.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(super) struct DelayPlan {
    /// Lignes autonomes à allouer : signal porté → géométrie requise.
    pub lines: BTreeMap<SigId, PlannedLine>,        // {max_delay, strategy, size, name}
    /// Métadonnées de taille des sorties de récursion (l'actuel rec_output_analysis).
    pub rec_outputs: BTreeMap<(u32, usize), DelayAnalysisEntry>,
}

/// Une passe, sans argument FirStore, renvoie des données sur lesquelles l'appelant peut assertir.
pub(super) fn plan_delays(
    arena: &TreeArena,
    sig_types: &HashMap<SigId, SigType>,
    signals: &[SigId],
    options: &DelayOptions,
) -> Result<DelayPlan, SignalFirError>;
```

L'émission est alors scindée en consommateurs purs du plan :

```
plan_delays(...)               // UN parcours, sans FIR        ← remplace analyze_* + scan_*
  └─► DelayPlan (immuable pour le reste de la construction)
        ├─ declare_lines(&plan, &mut DelayDecls)               // fVec*/iVec*, clears, fIOTA/fIdx*
        ├─ emit_read/emit_write(&plan[carried], &mut Emit)     // phase 2
        ├─ emit_sample_end(&plan, &mut Emit)                   // phase 3
        └─ le dimensionnement récursion lit plan.rec_outputs   // recursion.rs
```

Le parcours unifié porte *les deux* accumulateurs que suivent les deux parcours actuels (délai
accumulé sur le chemin pour les sorties de récursion **et** max possédé par porteur), il produit donc
les deux cartes en une passe.

**Ce qui devient plus simple.**
- *À lire :* un parcours au lieu de deux quasi-doublons (élimine F1) ; le passe-partout des enfants de
  liste est écrit une fois.
- *À documenter :* la frontière de phase est désormais un **type** — « `DelayPlan`, c'est tout ce qui
  est décidé avant qu'aucun FIR n'existe » est un invariant d'une phrase.
- *À tester :* les assertions passent de « grep le FIR généré pour `fVec42` » à
  `assert_eq!(plan.lines[&s], PlannedLine{ size: 8, strategy: CircularPow2, .. })`. Le plan est des
  données `PartialEq` ; les tests cessent de dépendre de l'émission.
- L'immuabilité de `DelayPlan` pendant l'abaissement rend la règle « décider une fois, puis lire
  seulement » (déjà l'intention, voir `delay_line_info` dans `state.rs`) impossible à violer.

**Composabilité / surface d'interaction.** Les phases interagissent via **une seule valeur
immuable**. La planification n'a pas de `FirStore` ; l'émission ne re-décide jamais. C'est
l'interface la plus simple possible entre étapes : une structure de données.

**Coût.** Unifier les deux stratégies de mémoïsation en un parcours demande du soin (la mémo de délai
accumulé et la mémo de max par porteur doivent coexister — toutes deux monotones, donc une seule
visite de nœud peut mettre à jour les deux). `recursion.rs` passe de
`delay.rec_output_analysis(var, i)` à `plan.rec_outputs[&(var,i)]`.

**Indépendance.** Indépendante de A et C. Elle réorganise les grappes 1/8 (analyse) et la plomberie
de phase ; elle ne se soucie pas de la manière dont une stratégie émet sa lecture (c'est A) ni de la
façon dont la math feuille est écrite (c'est C). S'empile proprement sur A : `PlannedLine.strategy`
devient l'`enum DelayKind` de A.

### Proposition C — Une fine couche arithmétique pour que les formules se lisent comme les docs (découpe en profondeur)

**Idée.** Attaquer F6 directement : introduire une aide d'expression *uniquement à la compilation* de
sorte que la math d'index s'écrive comme le commentaire de module la décrit déjà, et garder les
formules de chaque géométrie d'un seul coup d'œil.

```rust
// Un wrapper à coût nul portant (FirId, &mut FirStore) pour enchaîner les opérateurs.
let idx = e.iota() - amount;          //  construit Sub(load fIOTA, amount)
let masked = idx & (size - 1);        //  construit And(idx, mask)
// lecture if-wrapping, aujourd'hui 40 lignes, devient :
let raw = e.counter(name) + size - amount;
let read = raw.wrap_if_ge(size);      //  select2(raw>=size, raw-size, raw)
// avance du compteur :
let next = (e.counter(name) + 1).wrap_to_zero_if_ge(size);
```

De façon équivalente (si la surcharge d'opérateurs est jugée trop magique pour le style du dépôt),
une poignée de combinateurs nommés : `e.sub`, `e.mask(size)`, `e.wrap_if_ge(raw, size)`,
`e.bump_wrap(counter, size)`. Dans les deux cas, l'objectif est que `if_wrapping_read_index` et
`bump_if_wrapping_counter` se réduisent à ~5 lignes évidentes chacune, reflétant visiblement
`buf[(idx + size - N) replié]` et `idx = (idx+1 ≥ size) ? 0 : idx+1` de l'en-tête du module.

**Ce qui devient plus simple.**
- *À lire :* l'arithmétique DSP devient lisible et **vérifiable à l'œil contre le C++**
  `writeReadDelay` ; le ratio passe-partout/signal chute (~40 % de lignes en moins dans la grappe 7).
- *À documenter :* le code *est* la formule ; les commentaires n'ont plus à la redire.
- *À faire confiance :* moins de `let` intermédiaires ⇒ moins d'endroits où transposer un `+`/`−`.

**Composabilité / surface d'interaction.** Quasi nulle — c'est un utilitaire feuille pur qui produit
des `FirId`. Il se compose sous A (le `emit_*` de chaque stratégie l'utilise) et sous B (les
consommateurs d'émission l'utilisent) sans aucun couplage.

**Coût.** Une petite aide nouvelle à apprendre et à prouver correcte *une fois* ; doit compiler vers
exactement les mêmes appels `FirBuilder` (égalité FIR-témoin, §4). Le risque est concentré et facile
à verrouiller.

**Indépendance.** La plus indépendante des trois — elle peut atterrir en premier, seule, au risque le
plus faible, et elle rend lisibles les diffs de A et B.

#### Exemple travaillé — ce que « plus simple » apporte concrètement (preuve de concept)

La preuve la plus convaincante que le gain de lisibilité est réel et non cosmétique. Aujourd'hui
`if_wrapping_read_index` ([`delay.rs:1068`](../crates/transform/src/signal_fir/delay.rs)) dépense ~42
lignes de cérémonie `FirBuilder` pour encoder une formule d'une ligne. Ci-dessous, rendue fidèlement
dans sa structure, avec `B!{…}` représentant l'incantation répétée
`{ let mut b = FirBuilder::new(store); b.… }` :

```rust
// AVANT — delay.rs:1068 (42 lignes une fois les blocs B!{…} dépliés)
fn if_wrapping_read_index(store, counter_name, amount, size) -> FirId {
    let size_i32  = i32::try_from(size).unwrap_or(i32::MAX);
    let counter   = B!{ load_var(counter_name, Struct, Int32) };
    let size_fir  = B!{ int32(size_i32) };
    let plus_size = B!{ binop(Add, counter, size_fir, Int32) };
    let raw       = B!{ binop(Sub, plus_size, amount, Int32) };
    let cond      = B!{ binop(Ge, raw, B!{ int32(size_i32) }, Int32) };
    let adjusted  = B!{ binop(Sub, raw, B!{ int32(size_i32) }, Int32) };
    B!{ select2(cond, adjusted, raw, Int32) }
}
```

Sous la Proposition C, la même fonction se lit comme la formule que l'en-tête du module énonce déjà
(`buf[(idx + size − N) replié]`) :

```rust
// APRÈS — 3 lignes, FIR émis identique
fn if_wrapping_read_index(e: &mut Emit, counter: &str, amount: FirId, size: usize) -> FirId {
    let raw = e.counter(counter) + e.int(size) - amount;  // counter + size − amount
    raw.wrap_if_ge(size)                                  // select2(raw ≥ size, raw − size, raw)
}
```

`Emit` est un fin wrapper `(FirId, &mut FirStore)` dont les `+`/`-`/`&` construisent les mêmes nœuds
`FirBinOp` et dont `wrap_if_ge` construit le même `select2`. Le diff FIR-témoin (§4.2) reste vide ;
seules chutent le nombre de lignes — et l'effort du lecteur pour confirmer que la fonction
correspond au `writeReadDelay` du C++. `bump_if_wrapping_counter` et `masked_delay_index`
s'effondrent de la même manière.

### 3.x Comparaison côte à côte

| Axe | A — unité par stratégie | B — IR plan/émission | C — couche arithmétique |
|-----|-------------------------|----------------------|-------------------------|
| Friction principale levée | F2, F3, F4 | F1, F5 (couplage de phase) | F6 |
| Direction de découpe | verticale (par concept) | horizontale (par phase) | profondeur (par feuille) |
| Nouvel artefact | trait `DelayKind` + 3 fichiers | valeur `DelayPlan` + 1 passe | aide d'expression |
| Style de test débloqué | tests unitaires par stratégie | tests-données assert-sur-plan | égalité FIR-témoin |
| Surface d'interaction | 1 trait, 5 méthodes | 1 struct immuable | aucune (feuille) |
| Risque | moyen (dispatch) | moyen (unifier les parcours) | faible (local) |
| Indépendance | totale | totale | totale |
| Lignes déplacées/supprimées | ~250 réorganisées | ~140 dédupliquées | ~80 réduites |

**Combinaison & ordre recommandés** (chaque étape livre au vert isolément) :
1. **C d'abord** — risque le plus faible, pas de changement d'interface, rend lisibles les diffs
   suivants.
2. **B ensuite** — unifier les parcours derrière `DelayPlan` ; les tests deviennent des assertions de
   données, ce qui dérisque A.
3. **A en dernier** — avec les feuilles lisibles de C et le `PlannedLine.strategy` de B, les fichiers
   par stratégie s'écrivent presque seuls, et les branches impossibles (F3) s'évanouissent.

Si une seule est faite, faire **C** (gain de lisibilité le moins cher). Si deux, faire **C + B**
(élimine la plus grosse duplication structurelle, F1). Les trois ensemble donnent l'état final « un
fichier par concept, une passe, des formules qui se lisent comme la spec ».

---

## 4. Migrer de façon sûre et testable

Le contrat non négociable : **FIR émis identique** ⇒ C/WASM généré identique ⇒ performance
identique. Chaque étape ci-dessous est conditionnée à cela.

### 4.1 Le filet de sécurité qui existe déjà

- **71 fonctions `#[test]`** dans `signal_fir/tests.rs`, dont **~30 spécifiques aux délais** (shift
  d=1/2/3, circulaire à la frontière `-mcd`, if-wrapping à la frontière `-dlt`, montants
  variables/slider, passthrough à délai zéro, et toutes les formes de fusion de récursion). Elles
  assertent déjà sur la forme du FIR émis et sur `rec_output_analysis`.
- **L'oracle impulse-tests** (`tests/impulse-tests/` + `crates/impulse-runner`, voir
  `project_impulse_tests_harness`) : un véritable oracle C++ à 4 passes, référence **cpp 92/93**.
  Toute régression de délai qui change les échantillons d'exécution y apparaît.

### 4.2 Ajouter un point d'ancrage avant de toucher quoi que ce soit : un diff FIR-témoin

Caractériser par test la *sortie*, pas seulement le comportement. Construire un petit corpus de DSP
couvrant chaque stratégie et les cas de fusion, vidanger le FIR émis (les chemins
`dump_sig`/imprimeur-FIR déjà utilisés dans les tests) et le figer en instantané. Les refactorisations
doivent produire un vidage **identique au bit près** (ou identique en AST). Cela attrape les
divergences que les tests unitaires manquent et transforme « la performance a-t-elle changé ? » en
« le FIR a-t-il changé ? » — un contrôle mécanique.

### 4.3 Principes génériques

- **Déplacements préservant l'interface.** Garder stable la surface `pub(super)` (`DelayManager`,
  `DelayFirCtx`, `DelayLoweringCtx`, `emit_*_for_line`, les fns libres de dimensionnement) pour que
  les quatre sites d'appel de `module/` ne bougent pas pendant que l'intérieur se déplace. Ne changer
  les sites d'appel que dans l'étape dédiée qui retire une interface.
- **Un changement structurel par commit.** Ne jamais combiner un déplacement avec un ajustement de
  comportement ; chaque commit compile et passe `cargo test -p transform` **et** l'oracle impulse.
- **Test différentiel pendant une bascule.** Lors de l'introduction d'une implémentation parallèle (le
  nouveau `plan_delays`, un nouveau `DelayKind`), calculer *les deux* ancien et nouveau un moment et
  `debug_assert_eq!` qu'ils s'accordent sur le corpus ; ne supprimer l'ancien chemin qu'une fois qu'ils
  se sont accordés sur l'ensemble des tests.

### 4.4 Recette par proposition

**C (couche arithmétique) — atterrir en premier.**
1. Ajouter l'aide avec ses propres tests unitaires (chaque combinateur construit la forme `FirId`
   attendue).
2. Réécrire `masked_delay_index`, `if_wrapping_read_index`, `bump_if_wrapping_counter` et les aides de
   shift pour l'utiliser — *une aide par commit*, le diff FIR-témoin doit rester vide.
3. Aucun changement de site d'appel ; pur échange de feuille.

**B (plan/émission) — atterrir en deuxième.**
1. Écrire `plan_delays` *à côté* des `analyze_signals`/`scan_signals` existants ; ne pas les retirer.
2. Dans `prepare_delay_lines`, appeler les deux et `debug_assert_eq!` que `DelayPlan` reproduit la
   carte `max_delays` et les entrées `rec_output_analysis` d'aujourd'hui. Exécuter toute la suite +
   l'oracle.
3. Basculer `prepare_delay_lines`, `ensure_recursion_array_for_group` et les requêtes d'abaissement
   pour lire `DelayPlan` ; supprimer `analyze_*`/`scan_*` et la duplication
   `rec_output_analysis`/`delay_lines`.
4. Convertir les ~30 tests de délai qui grep le FIR pour le dimensionnement en `assert_eq!` sur
   `DelayPlan` là où c'est plus direct (optionnel, mais c'est le bénéfice).

**A (unités par stratégie) — atterrir en dernier.**
1. Introduire l'`enum DelayKind` + le trait ; l'implémenter en *déléguant aux fonctions actuelles*
   pour que le comportement soit inchangé. Diff FIR-témoin vide.
2. Porter une stratégie à la fois dans son propre fichier, la plus simple d'abord : **Shift →
   IfWrapping → CircularPow2**. Après chacune, supprimer le bras de cette stratégie de
   `emit_*_for_line` et ses branches `DelayRuntimeState` mortes (F3).
3. Quand les trois sont portées, retirer `RingDelayModel`, `DelayRuntimeState`,
   `runtime_state_for_line` et le dispatch dupliqué.

### 4.5 Pourquoi la performance est préservée (par construction)

- **Même FIR ⇒ même code machine.** Le diff FIR-témoin est la garantie ; les backends voient une
  entrée identique, donc le C/WASM/Cranelift généré est identique.
- **Le dispatch reste statique.** A utilise un `enum` + `match` (ou conserve le
  `RingDelayStrategyEmitter<M>` monomorphisé existant), jamais `dyn` — pas de vtable dans le chemin
  d'émission chaud. À noter : l'émission s'exécute à la *compilation* du DSP, pas dans la boucle
  audio, donc même `dyn` ne toucherait pas la vitesse DSP d'exécution ; le choix de l'`enum` est par
  principe de coût nul et pour l'inlining, pas pour la latence audio.
- **B est strictement moins de travail à la construction** (un parcours remplace deux et plus), et
  produit les mêmes déclarations et instructions.
- **C est du sucre uniquement à la compilation** qui s'abaisse vers les appels `FirBuilder`
  identiques ; `#[inline]` le garde gratuit même dans le binaire du compilateur.

---

## 5. Recommandation

Le fichier est déjà correct et raisonnablement factorisé ; c'est une expérience de *lisibilité*, pas
une chasse aux bugs. Le chemin à plus forte valeur-par-risque est **C → B → A** :

1. **C** achète une lisibilité immédiate à un risque quasi nul et rend tout ce qui suit plus facile à
   relire.
2. **B** supprime la plus grosse duplication structurelle (les deux parcours d'arbre, F1) et
   transforme les décisions de délai en données testables.
3. **A** effondre ensuite le concept de stratégie à cinq sites en un-fichier-par-stratégie et supprime
   les branches impossibles.

Arrêtez-vous après n'importe quelle étape et le fichier est strictement plus clair qu'aujourd'hui,
avec la suite de tests et l'oracle impulse prouvant le comportement — et le diff FIR-témoin prouvant
la performance — inchangés.
