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

## 6. Formes mutuellement récursives (récursion multi-sortie)

### 6.1 Le motif

La récursion mutuelle en Faust apparaît lorsque l'opérateur `~` connecte des
signaux multi-canaux. Exemple :

```faust
process = si.bus(2) ~ (*(0.5), *(0.25));
```

Ici les deux canaux se réinjectent mutuellement via le chemin de rétroaction
parallèle. Le noeud `Rec` a :
- `left` = `si.bus(2)` (2→2 : identité sur 2 canaux)
- `right` = `(*(0.5), *(0.25))` (2→2 : gain indépendant sur chaque canal)

### 6.2 Forme des signaux

Le groupe récursif a **2 corps** (un par canal de sortie) :

```
DEBRUIJNREC(
    body = list(
        delay1(proj(0, DEBRUIJNREF(1))) * 0.5 + input(0),   ← corps₀
        delay1(proj(1, DEBRUIJNREF(1))) * 0.25 + input(1)   ← corps₁
    )
)
```

Les deux corps référencent le même `DEBRUIJNREF(1)` mais sélectionnent des
projections différentes (`proj(0, ...)` et `proj(1, ...)`).

### 6.3 Cas général à N sorties

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

## 7. Cas de récursion dégénérée

### 7.1 Qu'est-ce qui rend une récursion « dégénérée » ?

Une récursion est dégénérée lorsque certains canaux de sortie du groupe récursif
**n'utilisent pas réellement la rétroaction**. Cela signifie que leur
`aperture` vaut 0 — ils ne contiennent aucune référence `DEBRUIJNREF`.

### 7.2 Le test d'aperture

La fonction `aperture(expr)` calcule le **niveau De Bruijn libre maximal**
dans une expression de signal :

| Noeud | Aperture |
|-------|----------|
| `DEBRUIJNREF(level)` | `level` |
| `DEBRUIJNREC(body)` | `aperture(body) - 1` (le lieur capture un niveau) |
| Tout autre noeud | `max(aperture(enfants))` |
| Feuille (pas de refs) | `0` |

Quand `aperture(corps_i) > 0`, le corps référence véritablement le groupe
récursif → il doit être émis comme `proj(i, group)`.

Quand `aperture(corps_i) == 0`, le corps n'a aucune dépendance récursive → il
peut être émis directement, en court-circuitant l'enveloppe récursive.

### 7.3 Pourquoi c'est important : le problème `proj(7, W)`

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

### 7.4 L'élimination de dégénérescence en C++

Le compilateur C++ va plus loin avec `inlineDegenerateRecursions()` : il
détecte que 7 des 8 corps ne sont pas récursifs, **les retire du groupe**, et
le réduit à un groupe à corps unique. Mais l'index de projection est conservé :

```
Avant : SYMREC([b₀, b₁, ..., b₇])  avec proj(7, W)
Après : SYMREC([b₇])                avec proj(7, W)  ← index 7, arité 1 !
```

Cela crée une **projection hors limites** : `proj(7, W)` sur un groupe
d'arité 1.

### 7.5 Le correctif Rust : `canonicalize_unary_rec_projections`

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

### 7.6 Était-ce strictement nécessaire ?

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

## 8. Pipeline complet : De Bruijn → symbolique → FIR

```
Arbre de boîtes (noeuds Rec)
         │
         ▼
    propagate_in_slot_env           ← encodage De Bruijn (ce document)
         │
         ▼
Arbre de signaux avec noeuds DEBRUIJNREC / DEBRUIJNREF
         │
         ▼
    de_bruijn_to_sym (tlib)         ← conversion en forme nommée
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

### Conversion : `de_bruijn_to_sym`

Pour chaque lieur `DEBRUIJNREC(body)` :
1. Allouer une variable symbolique fraîche `W0`, `W1`, ...
2. Substituer `DEBRUIJNREF(1)` dans le corps par `SYMREF(W0)`.
3. Envelopper sous la forme `SYMREC(W0, corps_converti)`.

```
DEBRUIJNREC(add(delay1(proj(0, DEBRUIJNREF(1))), input(0)))
    ↓ de_bruijn_to_sym
SYMREC(W0, add(delay1(proj(0, SYMREF(W0))), input(0)))
```

Cela produit des groupes récursifs nommés lisibles par un humain, adaptés au
backend FIR.

---

## 9. Discussion de conception : pourquoi ne pas canonicaliser dans `propagate` ?

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

## 10. Résumé des fonctions clés

| Fonction | Fichier | Rôle |
|----------|---------|------|
| `make_mem_sig_proj_list` | `propagate/lib.rs` | Initialise les `Ri` placeholders de rétroaction : `delay1(proj(i, DEBRUIJNREF(1)))` |
| `lift_signals` / `liftn` | `propagate/lib.rs` | Incrémente les niveaux De Bruijn pour éviter la capture par de nouveaux lieurs |
| `aperture` | `propagate/lib.rs` | Calcule le niveau De Bruijn libre maximal (0 = pas récursif) |
| `debruijn_rec` / `debruijn_ref` | `propagate/lib.rs` | Constructeurs pour les noeuds `DEBRUIJNREC` / `DEBRUIJNREF` |
| `de_bruijn_to_sym` | `tlib/recursion.rs` | Convertit le De Bruijn positionnel en `SYMREC`/`SYMREF` nommés |
| `canonicalize_unary_rec_projections` | `transform/signal_prepare.rs` | Normalise les groupes récursifs à corps unique vers le slot physique dense `0` pour le typage, la promotion et le lowering FIR |

---

## 11. Glossaire

- **Aperture** : le niveau De Bruijn libre maximal dans un sous-arbre. Si > 0, le sous-arbre contient des références récursives non liées.
- **Lieur** (`DEBRUIJNREC`) : introduit une portée récursive. Chaque lieur capture les références de niveau 1.
- **Récursion dégénérée** : un groupe récursif où certaines sorties ne dépendent pas réellement de la rétroaction. Leur aperture vaut 0.
- **Indice/niveau De Bruijn** : schéma de référence sans nom où l'entier compte les lieurs englobants entre la référence et son site de liaison.
- **Levée** (`liftn`) : incrémentation des niveaux De Bruijn des références libres pour préserver la liaison correcte lors de l'introduction d'un nouveau lieur.
- **Projection** (`proj(i, group)`) : sélectionne la i-ème sortie d'un groupe récursif multi-sortie.
- **SYMREC/SYMREF** : forme symbolique nommée de la récursion, produite par `de_bruijn_to_sym` à partir de la forme positionnelle De Bruijn.
