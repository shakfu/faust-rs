# Abaissement de la récursion : des boîtes `Rec` aux signaux via l'encodage De Bruijn

> Document de conception interne pour `faust-rs`.
> Code source : `crates/propagate/src/lib.rs`, `crates/tlib/src/recursion.rs`,
> `crates/transform/src/signal_prepare.rs`.

---

## 1. Contexte : l'opérateur `~` en Faust

En Faust, l'opérateur tilde (`~`) crée des boucles de rétroaction. Par exemple :

```faust
process = + ~ *(0.5);
```

Ceci décrit une rétroaction à un échantillon de retard : la sortie est
réinjectée, multipliée par 0.5, puis additionnée à l'entrée. Au niveau de
l'algèbre des boîtes, cela produit un noeud `Rec(left, right)` où :

- **left** est le corps principal (ici `+`, arité 2→1),
- **right** est le chemin de rétroaction (ici `*(0.5)`, arité 1→1).

La règle d'arité de la récursion est :

```
left  : Li → Lo
right : Ri → Ro        avec Ri ≤ Lo et Ro ≤ Li

Rec(left, right) : (Li - Ro) → Lo
```

Le chemin de rétroaction « emprunte » `Ro` entrées au corps principal et les
remplace par des signaux récursifs retardés, tandis que les `Li - Ro` entrées
restantes sont exposées comme entrées externes de la composition.

---

## 2. Notation De Bruijn — Principes

### 2.1 Le problème : nommer les variables récursives

Lors de l'abaissement des boîtes `Rec` en signaux, il faut exprimer « ce signal
fait référence à la sortie du groupe récursif auquel il appartient ». Les
variables nommées fonctionnent pour les cas simples, mais Faust supporte des
groupes récursifs **imbriqués** et **mutuellement** récursifs. Les variables
nommées nécessitent des règles de portée soigneuses (alpha-renommage, évitement
de capture). Les indices De Bruijn résolvent ce problème structurellement.

### 2.2 Indices De Bruijn en lambda-calcul (rappel)

En lambda-calcul standard, les indices De Bruijn remplacent les noms de
variables par des **nombres positionnels** comptant le nombre de lieurs
séparant la référence de son site de liaison :

```
λx. λy. x        devient    λ. λ. 2
λx. λy. y        devient    λ. λ. 1
λx. x            devient    λ. 1
```

L'intuition clé : **le niveau 1** signifie toujours « lié par le lieur
immédiatement englobant », **le niveau 2** signifie « lié par le lieur un cran
plus haut », etc. Cela rend l'égalité structurelle triviale — plus besoin
d'alpha-équivalence.

### 2.3 De Bruijn dans les arbres de signaux Faust

Faust adapte ce principe aux groupes récursifs de signaux avec deux types de
noeuds :

| Noeud | Notation | Signification |
|-------|----------|---------------|
| `DEBRUIJNREC(body)` | Le **lieur** | Enveloppe le corps d'un groupe récursif. Analogue à `λ` ou `μ`. |
| `DEBRUIJNREF(level)` | La **référence** | Pointe vers un lieur englobant. Niveau 1 = le plus interne. |

Un simple feedback `+ ~ *(0.5)` produit (schématiquement) :

```
DEBRUIJNREC(
    body = [add(delay1(proj(0, DEBRUIJNREF(1))), input(0))]
)
                         ↑
                         └── « Je fais référence à la sortie de
                              mon DEBRUIJNREC immédiatement englobant »
```

### 2.4 Pourquoi ne pas utiliser des variables nommées dès le départ ?

1. **Partage structurel** : la `TreeArena` interne les noeuds par identité
   structurelle. Les noeuds De Bruijn produisent des formes d'arbres
   déterministes indépendantes du contexte de nommage, maximisant le partage.
2. **Portée correcte par construction** : des opérateurs `~` imbriqués
   produisent des lieurs `DEBRUIJNREC` imbriqués ; les références internes
   pointent automatiquement vers la bonne portée grâce à leur numéro de
   niveau — aucune passe d'alpha-renommage nécessaire.
3. **Technique standard** : le compilateur Faust en C++ utilise le même
   encodage (`rec`/`ref` avec niveaux De Bruijn), donc le portage Rust
   préserve la parité structurelle.

---

## 3. L'algorithme d'abaissement de `Rec` étape par étape

Voici ce qui se passe dans `propagate_in_slot_env` lorsqu'il rencontre
`FlatNodeKind::Rec(left, right)` :

### Étape 1 — Vérification d'arité

```
left  : Li → Lo
right : Ri → Ro
requis : Ri ≤ Lo  ET  Ro ≤ Li
```

### Étape 2 — Initialisation des entrées de rétroaction (`make_mem_sig_proj_list`)

Pour chacun des `Ri` canaux de rétroaction, on crée un signal « placeholder
récursif » initial :

```
l0[i] = delay1(proj(i, DEBRUIJNREF(1)))       pour i dans 0..Ri
```

Cela signifie : « la i-ème entrée de rétroaction est l'échantillon précédent
(`delay1`) de la i-ème projection (`proj`) du groupe récursif que nous sommes
en train de définir (`DEBRUIJNREF(1)`) ».

### Étape 3 — Propagation du chemin de rétroaction

```
l1 = propagate(right, l0)
```

Le chemin de rétroaction `right` reçoit les placeholders récursifs et produit
`Ro` signaux de sortie.

### Étape 4 — Construction du vecteur d'entrée complet pour le corps

```
rec_inputs = l1 ++ lift(external_inputs)
```

Le corps `left` reçoit :
- D'abord les `Ro` signaux issus du chemin de rétroaction,
- Puis les `Li - Ro` entrées externes, **levées** d'un niveau De Bruijn.

La levée est critique : les signaux externes qui contiennent déjà des noeuds
`DEBRUIJNREF` provenant d'une récursion *extérieure* doivent voir leurs niveaux
incrémentés pour continuer à pointer vers le bon lieur externe après
l'introduction d'un nouveau lieur interne.

### Étape 5 — Levée de l'environnement de slots

```
slot_env' = { k → liftn(v, 1) | (k, v) ∈ slot_env }
```

Même raison : toute entrée de l'environnement de slots contenant des noeuds
`DEBRUIJNREF` doit être levée pour éviter la capture par le nouveau lieur
interne.

### Étape 6 — Propagation du corps

```
l2 = propagate(left, rec_inputs)    // avec slot_env'
```

Cela produit `Lo` signaux de sortie qui peuvent référencer `DEBRUIJNREF(1)`.

### Étape 7 — Enveloppement dans `DEBRUIJNREC` et projection

```
group = DEBRUIJNREC(list(l2[0], l2[1], ..., l2[Lo-1]))
```

Puis pour chaque signal de sortie `l2[i]` :

```
si aperture(l2[i]) > 0 :
    output[i] = proj(i, group)      // véritablement récursif — doit passer par le groupe
sinon :
    output[i] = l2[i]               // pas récursif — émission directe (cas dégénéré)
```

---

## 4. Schéma : flux de signaux pour `+ ~ *(0.5)`

```
                        ┌─────────────────────────────────────────────┐
                        │            DEBRUIJNREC (lieur)              │
                        │                                             │
                        │   ┌─────────────────────────────────┐       │
    input(0) ──────────►│──►│              add                │──►────│──► sortie
                        │   │                                 │       │
                        │   └──────────▲──────────────────────┘       │
                        │              │                              │
                        │       delay1(proj(0, DEBRUIJNREF(1)))       │
                        │              │              ▲                │
                        │              │              │                │
                        │              │         ┌────┘                │
                        │              │         │  « ma sortie       │
                        │              └──── *(0.5)   à l'index 0 »   │
                        │                    (chemin de rétroaction)   │
                        └─────────────────────────────────────────────┘
```

Le `DEBRUIJNREF(1)` est l'auto-référence : « le groupe dans lequel je suis ».
Le `proj(0, ...)` sélectionne le slot de sortie 0 de ce groupe.
Le `delay1(...)` fournit le retard d'un échantillon qui rend la rétroaction
causale.

---

## 5. Récursion imbriquée et niveaux De Bruijn

Considérons une rétroaction imbriquée :

```faust
process = (+ ~ *(0.5)) ~ *(0.25);
```

Cela produit deux lieurs `DEBRUIJNREC` imbriqués :

```
DEBRUIJNREC₂(                          ← lieur externe (niveau 2 depuis l'intérieur)
    body₂ = DEBRUIJNREC₁(              ← lieur interne (niveau 1 depuis l'intérieur)
        body₁ = add(
            delay1(proj(0, DEBRUIJNREF(1))),  ← réfère au groupe interne
            ...
        )
    ),
    ...delay1(proj(0, DEBRUIJNREF(1)))...  ← à cette position, réfère au groupe externe
)
```

À l'intérieur de `body₁` :
- `DEBRUIJNREF(1)` → groupe interne (DEBRUIJNREC₁)
- `DEBRUIJNREF(2)` → groupe externe (DEBRUIJNREC₂)

L'opération de **levée** (`liftn`) garantit que lorsque des signaux passent de
la portée externe à la portée interne, leurs niveaux de référence sont
incrémentés pour continuer à pointer vers le lieur externe.

```
liftn(DEBRUIJNREF(n), threshold=1) =
    si n < 1 : DEBRUIJNREF(n)    // lié dans cette portée → inchangé
    si n ≥ 1 : DEBRUIJNREF(n+1)  // libre → levé au-delà du nouveau lieur
```

---

## 6. Récursion multi-sortie et vraie récursion mutuelle

### 6.1 Le motif

Faust peut produire des groupes récursifs multi-sorties sans que les sorties
soient mutuellement récursives au sens strict. Exemple :

```faust
process = si.bus(2) ~ (*(0.5), *(0.25));
```

Ici chaque canal se réinjecte dans son propre slot via le chemin de
rétroaction parallèle. Le noeud `Rec` a :
- `left` = `si.bus(2)` (2→2 : identité sur 2 canaux)
- `right` = `(*(0.5), *(0.25))` (2→2 : gain indépendant sur chaque canal)

### 6.2 Forme des signaux

Le groupe récursif a **2 corps** (un par canal de sortie) :

```
DEBRUIJNREC(
    body = list(
        delay1(proj(0, DEBRUIJNREF(1))) * 0.5,   ← corps₀
        delay1(proj(1, DEBRUIJNREF(1))) * 0.25   ← corps₁
    )
)
```

Les deux corps référencent le même `DEBRUIJNREF(1)` mais sélectionnent chacun
leur propre projection (`proj(0, ...)` et `proj(1, ...)`). C'est bien un groupe
récursif multi-sortie, mais pas encore une vraie forme mutuellement récursive.

### 6.3 Vraie récursion mutuelle par croisement des voies de feedback

Une variante réellement mutuellement récursive croise les deux voies de
feedback :

```faust
import("stdfaust.lib");

process = si.bus(2) ~ ((*(0.5), *(0.25)) : ro.cross(2));
```

Le groupe récursif correspondant devient :

```
DEBRUIJNREC(
    body = list(
        delay1(proj(1, DEBRUIJNREF(1))) * 0.25,   ← corps₀ dépend de la sortie 1
        delay1(proj(0, DEBRUIJNREF(1))) * 0.5     ← corps₁ dépend de la sortie 0
    )
)
```

On a alors une vraie récursion mutuelle :

- la sortie 0 dépend de la sortie 1 ;
- la sortie 1 dépend de la sortie 0.

### 6.4 Cas général à N sorties

Pour une rétroaction à N canaux :

```
make_mem_sig_proj_list(N) produit :
    [ delay1(proj(0, DEBRUIJNREF(1))),
      delay1(proj(1, DEBRUIJNREF(1))),
      ...
      delay1(proj(N-1, DEBRUIJNREF(1))) ]
```

Chaque index de projection correspond à un slot dans la liste des corps du
groupe récursif. La forme enveloppée finale est :

```
group = DEBRUIJNREC(list(corps₀, corps₁, ..., corps_{N-1}))
output[i] = proj(i, group)     si aperture(corps_i) > 0
output[i] = corps_i            sinon
```

---

## 7. Aperture : mesurer l'ouverture récursive

### 7.1 Concept

L'**aperture** d'un sous-arbre de signaux est la profondeur maximale des
références De Bruijn libres (non liées) qu'il contient. Elle répond à une
question fondamentale : *cette expression dépend-elle d'un groupe récursif
englobant, et si oui, à combien de niveaux d'imbrication ?*

| Aperture | Signification |
|----------|---------------|
| `0` | L'expression est **fermée** — elle n'a pas de référence récursive libre. Elle peut être évaluée indépendamment de tout `DEBRUIJNREC` englobant. |
| `1` | L'expression référence son groupe récursif **immédiatement englobant** (`DEBRUIJNREF(1)`). |
| `2` | L'expression référence un groupe situé **deux niveaux d'imbrication au-dessus** (`DEBRUIJNREF(2)`). |
| `n` | L'expression atteint `n` niveaux de lieurs vers l'extérieur. |

C'est directement analogue au concept de **variables libres** en lambda-calcul :
un terme sans variables libres est fermé (un combinateur) ; un terme avec des
variables libres est ouvert et doit être évalué dans un contexte qui les lie.
L'aperture est l'équivalent De Bruijn — au lieu de traquer les *noms* de
variables, elle traque la *profondeur* de la référence non liée la plus
profonde.

### 7.2 Algorithme de calcul

L'aperture est calculée récursivement sur la structure de l'arbre avec trois
règles :

```
aperture(DEBRUIJNREF(level))  =  level
aperture(DEBRUIJNREC(body))   =  aperture(body) - 1
aperture(autre_noeud)         =  max(aperture(enfant) pour enfant dans enfants)
aperture(feuille)             =  0
```

**Règle 1 — Noeuds de référence** : un `DEBRUIJNREF(level)` contribue
exactement son niveau. C'est le cas de base qui introduit l'ouverture.

**Règle 2 — Noeuds lieurs** : un `DEBRUIJNREC(body)` *capture* un niveau de
référence. Si le corps a une aperture de 1 (référençant son propre lieur), le
résultat est 0 — le lieur l'a fermé. Si le corps a une aperture de 2
(atteignant un lieur externe), le résultat est 1 — un niveau libre subsiste.

**Règle 3 — Autres noeuds** : pour tout noeud composite (arithmétique, délai,
proj, ...), l'aperture est le maximum des apertures de ses enfants. Un seul
enfant ouvert suffit à rendre le parent ouvert.

### 7.3 Exemple détaillé

Considérons cette expression issue d'une récursion imbriquée :

```
add(
    delay1(proj(0, DEBRUIJNREF(1))),    ← aperture = 1
    mul(
        input(0),                        ← aperture = 0
        delay1(proj(0, DEBRUIJNREF(2)))  ← aperture = 2
    )                                    ← aperture = max(0, 2) = 2
)                                        ← aperture = max(1, 2) = 2
```

Si cette expression est enveloppée dans un `DEBRUIJNREC` :
```
DEBRUIJNREC(ci-dessus)  →  aperture = 2 - 1 = 1   (toujours ouverte — un niveau libre)
```

Si enveloppée dans deux `DEBRUIJNREC` imbriqués :
```
DEBRUIJNREC(DEBRUIJNREC(ci-dessus))  →  aperture = (2-1) - 1 = 0   (fermée)
```

### 7.4 Implémentation : C++ vs Rust

**C++ (`compiler/tlib/recursive-tree.cpp`)**

Dans le compilateur C++, l'aperture est un **champ synthétisé** stocké sur
chaque noeud (`CTree::fAperture`). Elle est calculée une seule fois lors de la
construction par `calcTreeAperture()` et mise en cache de manière permanente —
coût nul lors des lectures ultérieures :

```cpp
int CTree::calcTreeAperture(const Node& n, const tvec& br) {
    if (n == DEBRUIJNREF)   return int_value(br[0]);
    if (n == DEBRUIJN)      return br[0]->fAperture - 1;
    // sinon : max des enfants
    int rc = 0;
    for (auto& b : br) rc = max(rc, b->aperture());
    return rc;
}
```

Chaque noeud porte son aperture accessible via `tree->aperture()`, donc le
test lors de la propagation est une simple lecture de champ.

**Rust (`crates/tlib/src/recursion.rs`)**

Dans le compilateur Rust, les noeuds de `TreeArena` ne portent pas de champ
d'aperture pré-calculé. À la place, l'aperture est calculée à la demande et
mémoïsée dans un `AHashMap<TreeId, i64>`. L'implémentation unique vit dans
`tlib` :

```rust
fn aperture(arena: &TreeArena, root: TreeId, memo: &mut AHashMap<TreeId, i64>) -> i64 {
    if let Some(value) = memo.get(&root) { return *value; }
    let value = if let Some(level) = match_de_bruijn_ref(arena, root) {
        level
    } else if let Some(body) = match_de_bruijn_rec(arena, root) {
        aperture(arena, body, memo) - 1
    } else {
        arena.children(root).map_or(0, |ch|
            ch.iter().map(|&c| aperture(arena, *c, memo)).max().unwrap_or(0))
    };
    memo.insert(root, value);
    value
}
```

Deux points d'entrée publics partagent ce worker :
- `de_bruijn_aperture(arena, root)` — crée un cache local éphémère, adapté
  aux requêtes ponctuelles.
- `de_bruijn_aperture_with_memo(arena, root, memo)` — accepte un cache
  externe, utilisé par `propagate` pour amortir les coûts d'aperture sur
  l'ensemble du traversal de propagation (le cache est partagé avec `liftn`
  dans `PropagateMemo`).

### 7.5 Rôle dans le pipeline

L'aperture intervient à plusieurs points du pipeline de compilation :

1. **Lors de l'abaissement de `Rec` (étape 7)** : détermine quels corps d'un
   groupe récursif sont véritablement récursifs (`aperture > 0`) versus
   dégénérés (`aperture == 0`). Seuls les corps récursifs sont enveloppés dans
   `proj(i, group)`.

2. **Lors du `liftn`** : l'opération de levée utilise un seuil pour décider
   quelles références incrémenter. Une référence avec `level < seuil` est déjà
   liée dans la portée courante et ne doit pas être levée ; une référence avec
   `level >= seuil` est libre et doit être incrémentée. C'est intimement lié à
   l'aperture — `liftn` opère sur la même information structurelle.

3. **Lors de `de_bruijn_to_sym`** : la conversion de la forme positionnelle à
   la forme nommée utilise un raisonnement de type aperture pour déterminer
   quels noeuds `DEBRUIJNREF` sont capturés par un lieur `DEBRUIJNREC` donné
   (niveau 1) versus ceux qui atteignent un lieur externe (niveau > 1).

### 7.6 Diagramme de l'aperture

```
    DEBRUIJNREC                              aperture : max(1,2)-1 = 1
        │
        body = add(...)                      aperture : max(1,2) = 2
        ┌────────┴────────────┐
        │                     │
  delay1(proj(0,             mul(...)         aperture : max(0,2) = 2
    DEBRUIJNREF(1)))         ┌───┴───┐
        │                    │       │
    aperture : 1         input(0)  delay1(proj(0,
                         ap : 0      DEBRUIJNREF(2)))
                                         │
                                     aperture : 2
```

L'aperture se propage **vers le haut** depuis les feuilles (références) jusqu'à
la racine, et chaque lieur `DEBRUIJNREC` la **décrémente** de 1. Quand elle
atteint 0, le sous-arbre est fermé.

---

## 8. Cas de récursion dégénérée

### 8.1 Qu'est-ce qui rend une récursion « dégénérée » ?

Une récursion est dégénérée lorsque certains canaux de sortie du groupe récursif
**n'utilisent pas réellement la rétroaction**. Cela signifie que leur
`aperture` vaut 0 — ils ne contiennent aucune référence `DEBRUIJNREF`.

### 8.2 Le test d'aperture

La fonction d'aperture (voir [section 7](#7-aperture--mesurer-louverture-récursive)
pour le traitement complet) détermine quels corps sont véritablement récursifs :

- `aperture(corps_i) > 0` → le corps référence le groupe récursif → émettre
  comme `proj(i, group)`.
- `aperture(corps_i) == 0` → pas de dépendance récursive → émettre directement,
  en court-circuitant l'enveloppe récursive.

### 8.3 Pourquoi c'est important : le problème `proj(7, W)`

Considérons une rétroaction à 8 canaux où seul le canal 7 se réinjecte
réellement :

```faust
process = si.bus(8) ~ (!, !, !, !, !, !, !, *(gain));
```

Les canaux 0–6 ignorent leur entrée de rétroaction (`!`), ils ne sont donc pas
véritablement récursifs. Seul le canal 7 multiplie sa rétroaction par `gain`.

**Pendant la propagation** (dans `propagate_in_slot_env`), le test d'aperture
détecte cela à l'étape 7 :

```
pour i dans 0..8 :
    si aperture(l2[i]) > 0 :
        output[i] = proj(i, group)      // uniquement le canal 7
    sinon :
        output[i] = l2[i]               // canaux 0–6 : direct
```

Donc les canaux 0–6 sont émis directement (pas de `proj`), et seul le canal 7
passe par `proj(7, group)`.

### 8.4 L'élimination de dégénérescence en C++

Le compilateur C++ va plus loin avec `inlineDegenerateRecursions()` : il
détecte que 7 des 8 corps ne sont pas récursifs, **les retire du groupe**, et
le réduit à un groupe à corps unique. Mais l'index de projection est conservé :

```
Avant : SYMREC([b₀, b₁, ..., b₇])  avec proj(7, W)
Après : SYMREC([b₇])                avec proj(7, W)  ← index 7, arité 1 !
```

Cela crée une **projection hors limites** : `proj(7, W)` sur un groupe
d'arité 1.

### 8.5 Le correctif Rust : `canonicalize_unary_rec_projections`

Dans `signal_prepare.rs`, après que `de_bruijn_to_sym` convertit la forme
De Bruijn en forme symbolique, une passe de canonicalisation réécrit :

```
proj(k, group)  →  proj(0, group)     quand le groupe a exactement 1 corps
```

C'est un correctif plus restreint que l'élimination complète de dégénérescence
du C++. Il ne reconstruit pas le graphe de dépendances récursives ni ne réécrit
les définitions de projection. Il normalise simplement l'index une fois que
l'arité physique est connue comme étant 1.

**Déclencheur réel** : `re.zita_rev1_stereo(...)` (depuis `Birds.dsp`), une
réverbération algorithmique à 8 lignes de retard dont la matrice de
rétroaction produit exactement ce motif après évaluation.

### 8.6 Était-ce strictement nécessaire ?

Pas au sens sémantique absolu.

Des révisions plus anciennes du compilateur C++ existaient avant l’ajout de
`inlineDegenerateRecursions()`. Elles géraient déjà correctement les cas
récursifs dégénérés, mais sans réécrire l’arbre de signaux sous une forme
unaire canonique.

La stratégie historique était approximativement la suivante :

1. `propagate` émettait déjà directement les branches fermées quand
   `aperture == 0`, de sorte que seules les sorties vraiment récursives
   restaient sous forme de projections ;
2. les projections restantes pouvaient conserver leur index logique d’origine ;
3. le code de génération en aval tolérait cette forme au lieu d’exiger un IR
   récursif globalement normalisé.

Plus précisément :

- la génération scalaire compilait directement la définition de la projection
  demandée ;
- la génération de type instructions ne matérialisait que les projections
  effectivement utilisées.

Rust aurait donc pu choisir ce contrat plus souple lui aussi.

Cependant, le fast-lane Rust actuel choisit délibérément un invariant plus
dense : après conversion symbolique, un groupe récursif à corps unique doit
être adressé via le slot physique `0`.

Cela donne un IR préparé plus simple pour les passes en aval.

---

## 9. Conversion : De Bruijn vers forme symbolique (`de_bruijn_to_sym`)

### 9.1 Pourquoi une seconde représentation ?

Les indices De Bruijn sont idéaux lors de la construction (portée correcte par
construction, partage déterministe), mais ils sont opaques pour les passes
ultérieures : lire `DEBRUIJNREF(2)` nécessite de compter manuellement les
lieurs englobants. La forme symbolique remplace les niveaux positionnels par
des **variables nommées**, rendant les groupes récursifs auto-documentés et
plus faciles à traiter par le backend FIR.

| Forme De Bruijn | Forme symbolique |
|-----------------|------------------|
| `DEBRUIJNREC(body)` | `SYMREC(var, body)` |
| `DEBRUIJNREF(level)` | `SYMREF(var)` |

### 9.2 Vue d'ensemble de l'algorithme

La conversion est une traversée récursive en deux phases pour chaque lieur
`DEBRUIJNREC` rencontré :

```
fonction convert(noeud):
    si noeud est DEBRUIJNREC(body):
        var ← fresh_var()                     // allouer W0, W1, W2, ...
        body' ← substitute(body, level=1, remplacement=SYMREF(var))
        body'' ← convert(body')               // récurser dans le corps converti
        retourner SYMREC(var, body'')

    si noeud est DEBRUIJNREF(level):
        erreur("référence non liée")          // ne devrait pas arriver sur un arbre fermé

    si noeud est SYMREC ou SYMREF:
        passer inchangé

    sinon:
        retourner reconstruire(noeud, [convert(enfant) pour enfant dans enfants])
```

**Précondition** : l'arbre en entrée doit être **fermé** (`aperture ≤ 0`). Un
arbre ouvert laisserait des noeuds `DEBRUIJNREF` non résolus après conversion.
La fonction vérifie cela et retourne une erreur sinon.

### 9.3 Le helper `substitute`

L'opération clé est `substitute(arbre, level, remplacement)`, qui remplace
chaque `DEBRUIJNREF(level)` au niveau exact donné par le noeud de
remplacement :

```
fonction substitute(noeud, level, remplacement):
    si aperture(noeud) < level:
        retourner noeud                       // optimisation : aucune ref ne peut correspondre

    si noeud est DEBRUIJNREF(n):
        si n == level: retourner remplacement // c'est celle qu'on lie
        sinon:         retourner noeud        // appartient à un autre lieur

    si noeud est DEBRUIJNREC(body):
        retourner DEBRUIJNREC(substitute(body, level + 1, remplacement))
                                              // ↑ dans un lieur, la cible monte d'un cran

    sinon:
        retourner reconstruire(noeud, [substitute(enfant, level, remplacement)
                                       pour enfant dans enfants])
```

Le détail critique est le `level + 1` en descendant dans un `DEBRUIJNREC`
imbriqué : le niveau de référence cible se décale car le lieur interne
introduit une nouvelle portée. Cela garantit que seules les références
appartenant au lieur *courant* sont substituées.

Le **raccourci d'aperture** (`aperture(noeud) < level → retourner noeud`)
évite de traverser les sous-arbres qui ne peuvent pas contenir de référence
correspondante. C'est la principale optimisation de performance, partagée
entre les implémentations C++ et Rust.

### 9.4 Exemple détaillé : récursion simple

```
Entrée : DEBRUIJNREC(add(delay1(proj(0, DEBRUIJNREF(1))), input(0)))

Étape 1 : var = W0
Étape 2 : substitute(body, 1, SYMREF(W0))
           → add(delay1(proj(0, SYMREF(W0))), input(0))
Étape 3 : convert récursivement (plus de DEBRUIJNREC à l'intérieur)
           → pas de changement
Étape 4 : SYMREC(W0, add(delay1(proj(0, SYMREF(W0))), input(0)))
```

### 9.5 Exemple détaillé : récursion imbriquée

```
Entrée : DEBRUIJNREC(                              ← externe
             add(
                 DEBRUIJNREC(                      ← interne
                     mul(DEBRUIJNREF(1),           ← réfère à l'interne
                         DEBRUIJNREF(2))           ← réfère à l'externe
                 ),
                 DEBRUIJNREF(1)                    ← réfère à l'externe
             )
         )

Conversion externe :
  var_externe = W0
  substitute(body, 1, SYMREF(W0)) :
    - DEBRUIJNREF(1) dans add → SYMREF(W0)
    - Dans le DEBRUIJNREC interne : level passe à 2
      - DEBRUIJNREF(1) reste (niveau 1 ≠ 2) → réfère toujours à l'interne
      - DEBRUIJNREF(2) correspond au niveau 2 → SYMREF(W0)

  Après la substitution externe :
    add(
        DEBRUIJNREC(mul(DEBRUIJNREF(1), SYMREF(W0))),
        SYMREF(W0)
    )

  Récursion convert dans le DEBRUIJNREC interne :
    var_interne = W1
    substitute(mul(DEBRUIJNREF(1), SYMREF(W0)), 1, SYMREF(W1)) :
      - DEBRUIJNREF(1) → SYMREF(W1)
      - SYMREF(W0) → inchangé (pas un DEBRUIJNREF)

  Résultat final :
    SYMREC(W0, add(SYMREC(W1, mul(SYMREF(W1), SYMREF(W0))), SYMREF(W0)))
```

Chaque lieur obtient son propre nom unique. Les références sont maintenant
explicites — `W0` désigne toujours le groupe externe, `W1` toujours le groupe
interne, quelle que soit la profondeur d'imbrication.

### 9.6 Allocation de variables fraîches

C++ et Rust allouent les noms de variables à partir d'une séquence déterministe
`W0, W1, W2, ...`. L'allocation doit produire des noms qui ne collisionnent
pas avec les symboles pré-existants dans l'arène :

- **C++** : utilise `unique("W")`, qui génère un nom frais via un compteur
  global.
- **Rust** : itère le compteur d'index, tente d'interner `W{n}`, et saute
  tout nom déjà présent dans l'arène (détecté en vérifiant si `arena.len()` a
  augmenté après l'appel d'internement).

Cet évitement de collisions est nécessaire car l'arène peut déjà contenir des
symboles nommés `W0`, `W1`, ... provenant de code Faust évalué ou de passes
de conversion antérieures.

### 9.7 Mémoïsation et partage

Les deux implémentations préservent le partage structurel via la mémoïsation :

- **C++** : utilise les propriétés d'arbre (`setProperty`/`getProperty`) avec
  la clé `DEBRUIJN2SYM` pour la conversion et une clé composée
  `(SUBSTITUTE, level, remplacement)` pour la substitution.
- **Rust** : utilise trois caches `AHashMap` distincts dans la structure
  `Converter` : `convert_memo`, `substitute_memo`, et `aperture_memo`.

Ceci est critique pour la performance : l'arbre de signaux possède un partage
extensif (la `TreeArena` interne les sous-arbres structurellement identiques),
donc sans mémoïsation le même sous-arbre serait traversé un nombre
exponentiellement élevé de fois.

### 9.8 Implémentation : C++ vs Rust

**C++ (`compiler/tlib/recursive-tree.cpp`)**

```cpp
static Tree calcDeBruijn2Sym(Tree t) {
    Tree body, var;
    if (isRec(t, body)) {
        var = tree(unique("W"));
        return rec(var, deBruijn2Sym(substitute(body, 1, ref(var))));
    } else if (isRef(t, var)) {
        return t;                       // déjà symbolique
    } else {
        // reconstruire avec les enfants convertis
        tvec br(t->arity());
        for (int i = 0; i < t->arity(); i++)
            br[i] = deBruijn2Sym(t->branch(i));
        return tree(t->node(), br);
    }
}
```

**Rust (`crates/tlib/src/recursion.rs`)**

```rust
fn convert(&mut self, id: TreeId) -> Result<TreeId, RecursionError> {
    if let Some(mapped) = self.convert_memo.get(&id) { return Ok(*mapped); }

    if let Some(body) = match_de_bruijn_rec(self.arena, id) {
        let var = self.fresh_var();
        let replacement = sym_ref(self.arena, var);
        let substituted = self.substitute(body, 1, replacement)?;
        let converted_body = self.convert(substituted)?;
        let out = sym_rec(self.arena, var, converted_body);
        self.convert_memo.insert(id, out);
        return Ok(out);
    }
    // ... passthrough SYMREF, erreur DEBRUIJNREF, reconstruction générique
}
```

La version Rust diffère sur deux points :
1. Retourne `Result` avec des erreurs typées au lieu d'assertions
   (`faustassert`).
2. Conserve tous les caches dans une structure `Converter` unique (durée de vie
   limitée) plutôt que dans des propriétés d'arbre (durée de vie globale).

### 9.9 Diagramme : flux de conversion

```
    DEBRUIJNREC ──────────────────────────────────────► SYMREC(W0, ...)
         │                                                  │
         │  1. fresh_var() → W0                             │
         │  2. substitute(body, 1, SYMREF(W0))              │
         │  3. convert(corps_substitué)                     │
         │                                                  │
         ▼                                                  ▼
    DEBRUIJNREF(1) ─── substitution ────────────────► SYMREF(W0)
    DEBRUIJNREF(2) ─── inchangé (level ≠ 1) ───────► DEBRUIJNREF(2)
                       (sera traité par le convert externe)

    Autres noeuds ─── reconstruction avec enfants convertis ──► même structure
```

---

## 10. Pipeline complet : De Bruijn → symbolique → FIR

```
Arbre de boîtes (noeuds Rec)
         │
         ▼
    propagate_in_slot_env           ← encodage De Bruijn (sections 3–6)
         │
         ▼
Arbre de signaux avec noeuds DEBRUIJNREC / DEBRUIJNREF
         │
         ▼
    de_bruijn_to_sym (tlib)         ← conversion symbolique (section 9)
         │
         ▼
Arbre de signaux avec noeuds SYMREC(var, body) / SYMREF(var)
         │
         ▼
    canonicalize_unary_rec_projections (signal_prepare)
         │
         ▼
    signal_fir                      ← génération de code FIR
```

---

## 11. Discussion de conception : pourquoi ne pas canonicaliser dans `propagate` ?

Une question naturelle se pose : puisque `propagate` sait déjà quels canaux
sont dégénérés (via le test d'aperture à l'étape 7), pourrait-on effectuer la
canonicalisation à ce niveau au lieu de la reporter à `signal_prepare` ?

### Ce que `propagate` pourrait faire

À l'étape 7, `propagate` distingue déjà les corps récursifs (`aperture > 0`)
des non-récursifs (`aperture == 0`). Il pourrait aller plus loin :

1. Filtrer la liste des corps du groupe pour ne garder que les corps
   véritablement récursifs.
2. Construire un `DEBRUIJNREC` plus petit avec uniquement ces corps.
3. Renuméroter les indices de projection en indices denses (`0..N_récursifs`).

Cela éliminerait le cas dégénéré à la source, avant même que
`de_bruijn_to_sym` ne s'exécute.

### Pourquoi la conception actuelle le maintient dans `signal_prepare`

**Le problème ne provient pas de `propagate`.** Le `propagate` Rust construit
un groupe valide à 8 corps avec `proj(7, group)` — l'index 7 est dans les
limites (7 < 8). L'index hors limites n'apparaît *qu'après* la passe C++
`inlineDegenerateRecursions()` qui retire les 7 corps non-récursifs du groupe
tout en conservant l'index de projection original. La canonicalisation dans
`signal_prepare` est donc un **correctif de compatibilité** avec la forme
produite par le pipeline C++.

Le placement actuel est justifié par plusieurs facteurs :

1. **Opère sur `SYMREC`/`SYMREF`** : la canonicalisation travaille sur la
   forme symbolique, qui n'existe pas encore pendant la propagation (forme
   De Bruijn). Une version au niveau de la propagation nécessiterait une
   implémentation différente.

2. **Parité structurelle avec le C++** : `propagate` produit la même forme
   De Bruijn que le compilateur C++. Modifier la construction du groupe à ce
   stade divergerait de la structure C++ et compliquerait la vérification de
   parité.

3. **Séparation des responsabilités** : `propagate` traduit fidèlement la
   sémantique des boîtes en signaux. `signal_prepare` est le stage de
   normalisation avant FIR — l'endroit naturel pour les canonicalisations.

4. **Budget de complexité** : `propagate` est déjà dense. Y ajouter le
   filtrage de corps et la renumérotation d'indices augmente la surface
   d'erreur dans du code critique.

### Quand le déplacement aurait du sens

Si le projet venait à porter complètement `inlineDegenerateRecursions()` dans
le pipeline Rust (construction du graphe de dépendances récursives, réécriture
des définitions de projection via `hasProjDefinition`/`setProjDefinition`,
etc.), alors il serait pertinent de construire directement un groupe réduit
dans `propagate` plutôt que de construire un groupe complet puis de le réduire
ensuite. Ce serait plus efficace (un seul passage). Cependant, c'est un
chantier nettement plus conséquent que le correctif de compatibilité ciblé
actuel.

### État actuel du portage de `inlineDegenerateRecursions()`

`inlineDegenerateRecursions()` est une **passe du compilateur Faust C++
uniquement** — elle n'a **pas été portée en Rust**. Le pipeline Rust ne fait
pas :

- la construction du graphe de dépendances récursives,
- l'analyse des projections via `hasProjDefinition(...)` /
  `setProjDefinition(...)`,
- la réécriture des définitions de projection sous les délais,
- ni l'inlining des définitions de projection récursives tel que le font les
  règles de réécriture C++.

Le pipeline Rust suit à la place ce chemin simplifié :

1. **`propagate`** construit le groupe De Bruijn complet (les N corps, y
   compris les non-récursifs) — aucune élimination à ce stade.
2. **`signal_prepare`** convertit la forme De Bruijn en forme symbolique
   (`de_bruijn_to_sym`), puis applique le correctif restreint
   `canonicalize_unary_rec_projections` : quand un groupe symbolique a
   exactement 1 corps, tout index de projection qui le cible est réécrit à 0.

Ceci est explicitement documenté dans `signal_prepare.rs` (lignes 43–58)
comme une **normalisation de compatibilité**, pas un portage complet.

Ce correctif n’est d’ailleurs plus l’unique ligne de défense dans le code Rust.
Le lowerer FIR remappe lui aussi défensivement les groupes symboliques unaires
vers le slot `0`. En revanche, la canonicalisation au niveau de la préparation
garde une vraie valeur architecturale :

- l’inférence de types voit des indices physiques denses au lieu de
  `proj(7, unary_group)` ;
- la promotion peut continuer à raisonner sur un modèle dense de slots ;
- le lowering FIR n’a pas besoin de propager partout la distinction historique
  entre index logique et index physique.

Plus concrètement : avec le typeur Rust actuel, une projection hors bornes sur
un groupe symbolique unaire retomberait sinon sur un type maximal/imprécis au
lieu de réutiliser le type de son corps unique. La canonicalisation précoce
améliore donc non seulement la robustesse du lowering, mais aussi la précision
du typage.

**Le pipeline Rust a-t-il besoin de la passe complète ?** Actuellement, le
correctif restreint suffit pour tous les programmes rencontrés (notamment
`Birds.dsp` / `re.zita_rev1_stereo`). Cependant, si de futurs programmes
Faust produisent des groupes dégénérés réduits à N > 1 corps avec des indices
de projection non-denses, le portage complet deviendrait nécessaire.

### Où vit réellement la passe C++

Le compilateur C++ de référence réalise l’élimination complète dans :

- `compiler/transform/sigDegenerateRecursionElimination.hh`
- `compiler/transform/sigDegenerateRecursionElimination.cpp`
- fonction : `inlineDegenerateRecursions(Tree siglist, bool trace)`

et l’appelle plus tard depuis du code de génération, notamment dans :

- `compiler/generator/compile_scal.cpp`
- `compiler/generator/instructions_compiler.cpp`

Donc même côté C++, c’est bien une responsabilité de transform/generator, pas
de `propagate`.

### Rust aurait-il pu suivre l’ancienne approche C++ ?

Oui, mais cela aurait demandé un contrat d’IR différent.

Rust aurait dû accepter qu’une projection conserve un index logique qui n’est
plus égal à l’index physique du slot dans le groupe réduit. En pratique, cela
revient à enseigner ce cas spécial à plusieurs consommateurs en aval :

- l’inférence de types ;
- la promotion / normalisation ;
- le lowering FIR ;
- et toute passe ultérieure qui raisonne sur l’arité d’un groupe récursif.

La conception Rust actuelle préfère au contraire normaliser une fois dans
`signal_prepare`, puis laisser tout le reste de la pipeline raisonner
uniquement sur des slots physiques denses.

---

## 12. Résumé des fonctions clés

| Fonction | Fichier | Rôle |
|----------|---------|------|
| `make_mem_sig_proj_list` | `propagate/lib.rs` | Initialise les `Ri` placeholders de rétroaction : `delay1(proj(i, DEBRUIJNREF(1)))` |
| `lift_signals` / `liftn` | `propagate/lib.rs` | Incrémente les niveaux De Bruijn pour éviter la capture par de nouveaux lieurs |
| `de_bruijn_aperture` / `de_bruijn_aperture_with_memo` | `tlib/recursion.rs` | Calcule le niveau De Bruijn libre maximal (0 = fermé, >0 = récursif). La variante `_with_memo` accepte un cache externe pour usage amorti par `propagate`. Voir [section 7](#7-aperture--mesurer-louverture-récursive) |
| `debruijn_rec` / `debruijn_ref` | `propagate/lib.rs` | Constructeurs pour les noeuds `DEBRUIJNREC` / `DEBRUIJNREF` |
| `de_bruijn_to_sym` | `tlib/recursion.rs` | Convertit le De Bruijn positionnel en `SYMREC`/`SYMREF` nommés. Voir [section 9](#9-conversion--de-bruijn-vers-forme-symbolique-de_bruijn_to_sym) |
| `Converter::substitute` | `tlib/recursion.rs` | Remplace `DEBRUIJNREF(level)` par un remplacement symbolique, avec raccourci d'aperture |
| `Converter::fresh_var` | `tlib/recursion.rs` | Alloue des noms de variables symboliques sans collision (`W0`, `W1`, ...) |
| `canonicalize_unary_rec_projections` | `transform/signal_prepare.rs` | Normalise les groupes récursifs à corps unique vers le slot physique dense `0` pour le typage, la promotion et le lowering FIR |

---

## 13. Glossaire

- **Aperture** : le niveau De Bruijn libre maximal dans un sous-arbre. Si > 0, le sous-arbre est ouvert (contient des références récursives non liées) ; si 0, il est fermé. Analogue au comptage des variables libres en lambda-calcul. Calculée par trois règles : `DEBRUIJNREF(n)` → `n` ; `DEBRUIJNREC(body)` → `aperture(body) - 1` ; autres noeuds → `max(enfants)`. Voir [section 7](#7-aperture--mesurer-louverture-récursive).
- **Lieur** (`DEBRUIJNREC`) : introduit une portée récursive. Chaque lieur capture les références de niveau 1.
- **Récursion dégénérée** : un groupe récursif où certaines sorties ne dépendent pas réellement de la rétroaction. Leur aperture vaut 0.
- **Indice/niveau De Bruijn** : schéma de référence sans nom où l'entier compte les lieurs englobants entre la référence et son site de liaison.
- **Levée** (`liftn`) : incrémentation des niveaux De Bruijn des références libres pour préserver la liaison correcte lors de l'introduction d'un nouveau lieur.
- **Projection** (`proj(i, group)`) : sélectionne la i-ème sortie d'un groupe récursif multi-sortie.
- **Substitution** (`substitute`) : remplace tous les noeuds `DEBRUIJNREF` à un niveau donné par un noeud de remplacement. Descendre dans un `DEBRUIJNREC` imbriqué incrémente le niveau cible. Utilise le raccourci d'aperture pour ignorer les sous-arbres fermés.
- **SYMREC/SYMREF** : forme symbolique nommée de la récursion, produite par `de_bruijn_to_sym` à partir de la forme positionnelle De Bruijn. `SYMREC(var, body)` lie `var` dans `body` ; `SYMREF(var)` la référence. Voir [section 9](#9-conversion--de-bruijn-vers-forme-symbolique-de_bruijn_to_sym).
