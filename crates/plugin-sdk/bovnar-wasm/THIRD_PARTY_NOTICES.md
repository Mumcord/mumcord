# Third-party notices

This file has two parts. **Part 1** covers the six external unit vocabularies
whose identifiers are compiled into the library itself. **Part 2** covers
everything else this project did not author — the webfonts and libraries the
website serves, the imagery, the toolchain whose output is committed, one
algorithm the C sources derive from, and the code of conduct's adapted text.

Part 2 is the part that grows. It began as the website's components and has
twice been widened by a review that found a category nobody had thought of, so
it is written as an inventory rather than a list to be recited elsewhere: **the
sections of Part 2 are the authoritative enumeration**, and prose that summarises
them, here or in `LICENSE`, says "and everything else in Part 2" rather than
naming them, because a recited list is a list that goes stale.

The MIT grant in [`LICENSE`](LICENSE) clause 1 covers **this project's own
contribution**: the native unit registry, the `.bovnar` translation targets, the
refusal rationales, the generators, and every line of C, Python and CMake around
them. It does not, and cannot, grant rights in third-party material. See
`LICENSE` clause 4 for the vocabularies and clause 5 for everything else.

---

# Part 1 — Unit vocabularies

The unit-profile tables in `src/gendata/` — and the C tables generated from them
into the library, the amalgamation, the WebAssembly build, the Python wheel and
the Rust crate —
carry identifier strings from **six external vocabularies**. This part records,
for each, what was taken, which published version the table is verified against,
under which licence, and what remains open.

## What is taken, and what is not

**Taken:** identifier strings — a UCUM atom code, a QUDT or OM local name, a
UN/ECE common code, a UDUNITS spelling, a CF standard name. Alongside them, two
kinds of short upstream-derived text, both stated here rather than glossed over:

* **The conventional NAME of a unit**, where a row would otherwise be
  unreadable. A three-letter Rec 20 code is unintelligible without one, so most
  rows of `unece.bvnr` carry a trailing comment such as `# statute mile` or
  `# barrel, US petroleum`; the refusal strings in `ucum.bvnr` do the same, and a
  few of those match UCUM's own `<name>` field word for word (`%[slope]` is
  "percent of slope" in both). These are short factual designations of physical
  units, not the expressive content of any publication, and no mapping table can
  be written or reviewed without them.
* **CF's `canonical_units` field**, which `cf.bvnr` carries in a trailing comment
  on every row and translates into that row's target. §17.2 of
  `doc/11_bovnar_unit_profiles.md` is built on it: it is *what* a `cf:` name
  translates to, so it cannot be paraphrased away.

**Not taken:** no upstream definition, description, annotation, property,
`printSymbol` or explanatory prose. Where a `.bvnr` row carries reasoning — why a
code is refused, why a target is the one it is — that reasoning was written here.
No upstream conversion factor is copied either: every target resolves through
Bovnar's own registry, which `check_profile_factors.py` then compares against the
publisher's value rather than importing it.

**Not redistributed:** no upstream artefact ships with this project, and none is
tracked by git. The publishers' machine-readable definitions are fetched by
`check_profile_factors.py --fetch` into `build/vocab/`, which is git-ignored,
used to verify the tables against their sources, and never packaged. A release
tarball, a wheel, a `.wasm` and the amalgamation contain none of them.

**Adapted, in every case.** No table here reproduces a vocabulary. Each one
extracts that vocabulary's identifiers, maps them onto Bovnar's own unit
registry, and omits every code whose value this project cannot state exactly —
`doc/11_bovnar_unit_profiles.md` §6.4 and §18 record which, and why. Where the
licence below requires a modification notice, this paragraph is it.

**On "verified against".** Each entry below names the published version the table
is currently proved against by `check_profile_factors.py`, and the date that
version was retrieved into the verification cache. It is a statement about what
the rows demonstrably agree with today — which is the checkable claim, and the
one that matters for attribution.

---

## UCUM — Unified Code for Units of Measure

| | |
|---|---|
| **Used in** | `src/gendata/ucum.bvnr`, the `ucum:` profile |
| **Verified against** | UCUM 2.2, revision date 2024-06-17 (`ucum-essence.xml`), retrieved 2026-07-30 |
| **Source** | <https://github.com/ucum-org/ucum> |
| **Home** | <https://ucum.org/> |
| **Extracted** | atom codes, prefix spellings, the codes named in the refusal list, and the conventional name of a unit in a refusal string |

> Copyright ©1999-2024, Regenstrief Institute, Inc. All rights reserved.
>
> This product includes material from the Unified Code for Units of Measure
> (UCUM), used under the UCUM Copyright Notice and License, Version 1.1
> (June 2024). The full licence is at <https://ucum.org/license>.
>
> THE WORK IS PROVIDED ON AN "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF
> ANY KIND, either express or implied.
>
> Section 6 of that licence grants no permission to use the Licensor's trade
> names, trademarks, service marks or product names, beyond reasonable and
> customary use in describing the origin of the Work. Nothing here is such a
> use, and no affiliation with or endorsement by Regenstrief Institute is
> claimed or implied.

**Status: permission has not yet been obtained, and this is the one entry here
that needs it.** The UCUM licence is revocable, and its section 3(a) conditions the
grant on not creating derivative works of the Work and not adding to, deleting
from or modifying its content. `ucum.bvnr` extracts a subset of the atom table
and maps it onto a different unit model, which is a derivative work on the
licence's own definition (section 1.3), whatever its purpose. Section 2 permits developing
"Software Applications that will communicate with and interoperate with the
Work", which is exactly what the `ucum:` namespace is for — but that grant reads
to a program that consumes UCUM, not obviously to a re-encoded table shipped
alongside one.

Two things are true and worth stating plainly, because section 3(a) also bars using the
Work to develop or promulgate a different standard for identifying units of
measure, or to dilute UCUM's purpose:

* Bovnar's native unit notation was designed and specified independently of
  UCUM, is not offered as a replacement for it, and is not derived from it. The
  `ucum:` namespace exists so that a producer holding a UCUM code can write it
  down unchanged rather than translating it by hand first.
* A UCUM code is never silently reinterpreted. A code this project cannot carry
  exactly is refused — `error_unit_profile_unsupported` if UCUM defines it and
  Bovnar cannot represent it, `error_unit_illegal` if UCUM does not define it —
  and never approximated onto a nearby unit.

**Open action:** request written permission from Regenstrief Institute to
distribute this derived mapping table. Until that is granted, the `ucum:`
profile can be excluded from any build with `-DBVNR_WITH_UCUM_PROFILE=OFF`
(`doc/11` §9.4), which drops the table from the library entirely; the Rust crate
drops it when its `ucum` feature is off.

---

## QUDT — Quantities, Units, Dimensions and Data Types

| | |
|---|---|
| **Used in** | `src/gendata/qudt.bvnr`, `src/gendata/qudt-qk.bvnr`, `src/gendata/unece.bvnr` |
| **Verified against** | QUDT 3.1.0 (`owl:versionIRI <http://qudt.org/3.1.0/vocab/unit>`), retrieved 2026-07-30 |
| **Source** | <https://qudt.org/3.1.0/vocab/unit> and <https://qudt.org/3.1.0/vocab/quantitykind> |
| **Home** | <https://qudt.org/> |
| **Extracted** | unit local names, quantity-kind local names, and the `qudt:uneceCommonCode` cross-reference |

> Contains information from the QUDT Ontologies, which is made available under
> the Creative Commons Attribution 4.0 International License (CC BY 4.0),
> <https://creativecommons.org/licenses/by/4.0/>. Attribution is made to
> QUDT.org.
>
> **Modified.** Unit and quantity-kind local names were extracted and mapped
> onto Bovnar's native unit registry; names whose value this project cannot
> state exactly were omitted. QUDT.org does not endorse this project or its use
> of the material.

CC BY 4.0 section 2(a)(1) licenses the sui generis database rights as well as the
copyright, so the extraction is covered whichever right applies. There is no
ShareAlike term: the attribution above is the whole of the obligation, and it
imposes nothing on this project's own MIT-licensed code.

---

## OM 2 — Ontology of units of Measure

| | |
|---|---|
| **Used in** | `src/gendata/om.bvnr`, the `om:` profile |
| **Verified against** | OM 2.0 (`om-2.0.rdf`), retrieved 2026-07-30 |
| **Source** | <https://github.com/HajoRijgersberg/OM> |
| **Home** | <http://www.ontology-of-units-of-measure.org/> |
| **Extracted** | unit local names, and the unit compositions (prefix/base, numerator/denominator, term/term, base/exponent) from which each translation target was derived |

> (c) 2005-2025 Hajo Rijgersberg, Don Willems, Jan Top, Wageningen University
> and Research Centre, The Netherlands. Used under the Creative Commons
> Attribution 4.0 International License (CC BY 4.0),
> <https://creativecommons.org/licenses/by/4.0/>.
>
> **Modified.** Local names were extracted and their translation targets built
> from OM's own stated composition; units whose value this project cannot state
> exactly were omitted. The authors do not endorse this project or its use of
> the material.

OM is published by an EEA maker, so the EU sui generis database right
(sections 87a ff. UrhG) is the relevant right alongside copyright. CC BY 4.0
section 2(a)(1)
licenses it expressly, so the extraction is covered.

---

## UDUNITS-2 — Unidata units library

| | |
|---|---|
| **Used in** | `src/gendata/udunits.bvnr` (the `udunits:` profile), and `src/gendata/cf.bvnr` indirectly — CF states its `canonical_units` in UDUNITS syntax |
| **Verified against** | `master` branch (`udunits2-base.xml`, `-derived.xml`, `-accepted.xml`, `-common.xml`), retrieved 2026-07-30 |
| **Source** | <https://github.com/Unidata/UDUNITS-2> |
| **Home** | <https://www.unidata.ucar.edu/software/udunits/> |
| **Extracted** | unit symbols, singular and plural spellings, and prefix spellings |

The UDUNITS-2 licence requires this notice to be reproduced in source
distributions and in the documentation accompanying binary distributions. It is
reproduced here in full:

> Copyright 2025 University Corporation for Atmospheric Research and contributors.
> All rights reserved.
>
> This software was developed by the Unidata Program Center of the
> University Corporation for Atmospheric Research (UCAR)
> <http://www.unidata.ucar.edu>.
>
> Redistribution and use in source and binary forms, with or without modification,
> are permitted provided that the following conditions are met:
>
>    1) Redistributions of source code must retain the above copyright notice,
>       this list of conditions and the following disclaimer.
>    2) Redistributions in binary form must reproduce the above copyright notice,
>       this list of conditions and the following disclaimer in the documentation
>       and/or other materials provided with the distribution.
>    3) Neither the names of the development group, the copyright holders, nor the
>       names of contributors may be used to endorse or promote products derived
>       from this software without specific prior written permission.
>    4) This license shall terminate automatically and you may no longer exercise
>       any of the rights granted to you by this license as of the date you
>       commence an action, including a cross-claim or counterclaim, against
>       the copyright holders or any contributor alleging that this software
>       infringes a patent. This termination provision shall not apply for an
>       action alleging patent infringement by combinations of this software with
>       other software or hardware.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
> FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE CONTRIBUTORS
> OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
> WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
> CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS WITH THE SOFTWARE.

Clause 3 is why nothing in this project claims Unidata's or UCAR's endorsement.

---

## CF — Climate and Forecast standard names

| | |
|---|---|
| **Used in** | `src/gendata/cf.bvnr`, the `cf:` profile |
| **Verified against** | CF standard name table v94, last modified 2026-06-09, retrieved 2026-07-30 |
| **Source** | <https://cfconventions.org/Data/cf-standard-names/94/src/cf-standard-name-table.xml> |
| **Home** | <https://cfconventions.org/> |
| **Extracted** | standard names, and the `canonical_units` each name states |

> Contains standard names from the CF standard name table, maintained on behalf
> of the CF community by the Centre for Environmental Data Analysis (CEDA).
>
> **Modified.** Standard names were extracted and each translated to the unit
> its `canonical_units` field states; names whose canonical units this project
> cannot represent were omitted. The profile is read-only — a unit is never
> written back as a standard name, because a standard name asserts what the
> quantity *is* and a unit does not know that.

**Status: no licence is stated by the publisher.** The CF *conventions document*
is dedicated to the public domain under CC0 1.0
(<https://github.com/cf-convention/cf-conventions>), but the standard name table
is maintained separately, in `cf-convention/vocabularies`, which carries no
`LICENSE` file and no in-band rights statement. Nothing suggests the CF
community intends the table to be restricted — it exists to be embedded in data
files worldwide — but there is no grant to point at.

**Open action:** ask the CF community to declare an explicit licence for the
standard name table, proposing CC0 1.0 to match the conventions document. Until
then this attribution stands in its place, and the `cf:` profile can be excluded
from any build with `-DBVNR_WITH_CF_PROFILE=OFF` (in the Rust crate, with its
`cf` feature off).

---

## UN/ECE — Recommendations 20 and 21

| | |
|---|---|
| **Used in** | `src/gendata/unece.bvnr`, the `unece:` profile |
| **Vocabulary** | Recommendation No. 20, *Codes for Units of Measure Used in International Trade*, and Recommendation No. 21, *Codes for Passengers, Types of Cargo, Packages and Packaging Materials* |
| **Verified against** | QUDT 3.1.0's `qudt:uneceCommonCode` cross-reference — see below |
| **Home** | <https://unece.org/trade/uncefact/cl-recommendations> |
| **Extracted** | UN/ECE common codes, and the conventional name of a unit in a trailing comment on most rows |

> Contains UN/ECE common codes from UN/CEFACT Recommendations 20 and 21,
> published by the United Nations Economic Commission for Europe. Neither UNECE
> nor UN/CEFACT endorses this project or its use of the codes.

**Verified at one remove, through QUDT.** Recommendation 20 states its conversion
factors in prose, so there is no machine-readable publication to resolve against.
Every row of `unece.bvnr` is therefore proved against QUDT's published
`uneceCommonCode` cross-reference rather than against a UNECE artefact
(`check_profile_factors.py`, class `Unece`), and that cross-reference — not this
project's reading of a code list — is what decided the table's contents when it
was closed: `git log src/gendata/unece.bvnr` records rows being retargeted, and
others left out, on the strength of QUDT's label for the unit carrying the code.

Nothing in this repository reads or ships a UNECE publication; `check_profile_factors.py`
lists no UNECE source, and no UNECE file is tracked by git. What this project
cannot state is where each code was *first* encountered by its author over the
months the table was written by hand, which is why nothing here claims a UNECE
document was never consulted. What is UNECE's in the result is the codes.

**Status: formally unresolved, practically low risk.** The generic United
Nations website terms permit only personal, non-commercial download, without a
right to redistribute or to create derivative works. Those terms plainly are not
aimed at the code lists: Rec 20 exists to be implemented, and is implemented in
UN/EDIFACT, UBL, Peppol/EN 16931, ISO 20022, OPC UA and QUDT among many others.
The operative instrument is the UN/CEFACT Intellectual Property Rights Policy,
under which UN/CEFACT deliverables are made available for implementation
royalty-free.

**Open action:** obtain written confirmation of the reuse terms from
<cefact@unece.org>, and record it here.

---

## Other reference data

Two further external sources are used, neither of them a unit vocabulary and
neither raising the questions above:

* **ISO 4217** currency codes, numeric codes and minor-unit digits, in
  `src/gendata/currencies.bvnr`. The three-letter code list is published by the
  ISO 4217 Maintenance Agency (SIX Group) for free use, and codes and numeric
  identifiers are facts rather than expression. Cryptocurrency tickers are not
  ISO 4217 and are conventional.
* **CODATA 2022** for the one measured constant in `src/gendata/units.bvnr` —
  the atomic mass constant behind `Da`/`u`/`amu`. CODATA recommended values are
  published for unrestricted use; the edition is named in that file's header
  because a value without its edition cannot be checked. Every other factor in
  the registry is exact by definition or by convention.

---

# Part 2 — Everything else

The vocabularies above are the material this project *reasoned* about. They are
not the only third-party material it ships, and the sections below are the
inventory of the rest. It started as the website's components; a later review
added an algorithm the C sources derive from, and a later one still the adapted
text of the code of conduct. Each section is a notice, and where a licence asks
for more than a notice, it says so.

Most of what follows is plainly permissively licensed and needs only attribution.
**One entry is not:** the Contributor Covenant's text carries no licence
statement this project could establish, which is the same shape of question the
CF standard name table raises in Part 1, and it is recorded the same way — as
open, not as settled.

## Webfonts — IBM Plex Sans, JetBrains Mono

| | |
|---|---|
| **Files** | `web/fonts/ibm-plex-sans-*.woff2`, `web/fonts/jetbrains-mono-*.woff2` |
| **Licence** | SIL Open Font License, Version 1.1 (both) |

> Copyright © 2017 IBM Corp. with Reserved Font Name "Plex" —
> <https://github.com/IBM/plex>
>
> Copyright 2020 The JetBrains Mono Project Authors —
> <https://github.com/JetBrains/JetBrainsMono>

The OFL requires that **every copy of the Font Software carry the copyright
notice and the licence**, and a browser downloading a `.woff2` from the site is
receiving a copy. That is why [`web/fonts/OFL.txt`](web/fonts/OFL.txt) exists
and is uploaded with the rest of `web/`: a link from a CSS comment would not
have satisfied it.

Both files are **subsets** (latin, greek) of the upstream variable fonts,
generated from the Google Fonts css2 API and rehosted so the site has no
`fonts.googleapis`/`gstatic` dependency. Subsetting makes them Modified Versions
under OFL section 2; neither Reserved Font Name is changed, and neither font is sold on
its own.

## highlight.js 11.9.0

| | |
|---|---|
| **File** | `web/highlight-11.9.0.min.js` |
| **Licence** | BSD 3-Clause — <https://github.com/highlightjs/highlight.js/blob/main/LICENSE> |

> Copyright (c) 2006, Ivan Sagalaev. Highlight.js v11.9.0 (git: f47103d4f1),
> licensed under BSD-3-Clause.

The minified file retains its own `/*! … */` banner, which is what carries the
notice to anyone who fetches it — but that banner reads "(c) 2006-2023 undefined
and other contributors", an upstream build slip that names no copyright holder
at all. The line quoted above is the one from the project's own `LICENSE`, which
is why this entry states it rather than repeating the banner. Neither the
copyright holder's nor the contributors' names are used to endorse this project.

## marked 9.1.6

| | |
|---|---|
| **File** | `web/marked-9.1.6.min.js` |
| **Licence** | MIT — <https://github.com/markedjs/marked/blob/master/LICENSE> |

> marked v9.1.6 — a markdown parser. Copyright (c) 2011-2023, Christopher
> Jeffrey. (MIT Licensed) — <https://github.com/markedjs/marked>

The minified file retains the copyright line but not the full MIT permission
text, which the licence asks to accompany substantial portions. The upstream
`LICENSE` linked above is that text, and this entry is the pointer to it.

## Earth imagery — NASA

| | |
|---|---|
| **Files** | `web/earth.jpg` (Blue Marble, cloud-free, equirectangular), `web/earth_night.jpg` (Black Marble night lights) |
| **Status** | Not subject to copyright in the United States |

> Images courtesy of NASA. NASA does not endorse this project.

NASA media are generally not copyrighted and may be reused, including
commercially, with NASA credited as the source. Credit is the whole of the
obligation, and it is given here and — because a comment in a source file is not
a credit anyone reading the site can see — publicly, in the *Bildnachweis*
section of <https://www.bovnar.io/impressum.html>.

## Emscripten

| | |
|---|---|
| **Affects** | `web/bovnar_wasm_core.js`, `dist/wasm/bovnar*.mjs` and the committed `.wasm` — all `emcc` output (`wasm/build_wasm.sh`) |
| **Licence** | dual MIT / University of Illinois NCSA — <https://github.com/emscripten-core/emscripten/blob/main/LICENSE> |

> Portions of the generated JavaScript glue are Emscripten runtime code,
> copyright the Emscripten authors. Emscripten is dual-licensed under the MIT
> licence and the University of Illinois/NCSA Open Source License; it is used
> here under the MIT option.

The C that Emscripten compiles is this project's own; what is Emscripten's is
the loader and runtime shims it emits around it. Its licence expressly permits
distributing that output, and asks for the notice this entry gives.

## Date algorithms — Howard Hinnant

| | |
|---|---|
| **Affects** | `include/bvn_gregorian_date.h`, `src/utils/bvn_gregorian_date.c` |
| **Status** | Donated to the public domain by the author |

> The civil-date ↔ day-number conversions are derived from Howard Hinnant's
> *chrono-Compatible Low-Level Date Algorithms*,
> <https://howardhinnant.github.io/date_algorithms.html>, of which the author
> writes: "Consider these donated to the public domain."

This is the only algorithm in the C sources derived from a published
implementation rather than from a specification or from first principles, and
`bvn_gregorian_date.h` has credited it in its header since it was written.
There is no obligation attached; it is recorded here because a notices file that
lists a webfont and omits an algorithm is not telling the whole story.

The claim that it is the *only* one is bounded, not absolute: it rests on a
sweep of every source comment in `src/`, `include/` and `python/bovnar/` for
external URLs and for the phrases a derivation is normally marked with, which
found this and nothing else.

## Contributor Covenant — CODE_OF_CONDUCT.md

| | |
|---|---|
| **File** | `CODE_OF_CONDUCT.md` |
| **Adapted from** | Contributor Covenant, version 2.1 |
| **Source** | <https://www.contributor-covenant.org/version/2/1/code_of_conduct.html> |

> This Code of Conduct is adapted from the Contributor Covenant, version 2.1.
> The Community Impact Guidelines in it were inspired by Mozilla's code of
> conduct enforcement ladder.

That attribution is the one the Contributor Covenant supplies with the text, and
the file has carried it in its own Attribution section since it was added. It is
listed here so that every piece of adapted third-party text in the tree is in one
inventory rather than only in the file it lives in.

**The licence of the text is not clearly stated by its publisher.** The
`EthicalSource/contributor_covenant` repository carries the Hippocratic License
3.0, which covers the website's source; the version 2.1 page states no licence
for the code-of-conduct text itself, and neither does the text. Older
descriptions of the project as CC BY 4.0 are widely repeated and could not be
confirmed against anything the publisher says today, so this entry does not
repeat them. What is certain is that the Covenant asks to be attributed when
adapted, and it is.

## Not third-party, despite appearances

* `web/bovnar_highlight.js`, `web/bovnar_wasm.js`, `web/bovnar_parser_wasm.js`
  and the editor definitions under `highlighter/` — the Sublime syntax and
  colour scheme, the VS Code grammar and theme, the Vim, Geany and CLion
  definitions — are **clean-room original work**, written for this project
  against the Bovnar grammar rather than adapted from any existing syntax
  definition, theme or template.
* `dist/linguist/languages.yml.fragment` and `dist/mime/*` are likewise original
  and written *for* those projects rather than extracted from them: a fragment
  to be contributed upstream, not a copy of anything upstream holds.
* Development-time dependencies that are never redistributed — `pint`, `numpy`,
  `markdown`, `emsdk`, CMake, the compilers — are outside the scope of this
  file, which covers only what this project actually ships.

---

## Keeping this file true

* Each `src/gendata/*.bvnr` profile file repeats its own provenance block in its
  header, so the version and licence travel with the data rather than only with
  this file.
* `check_profile_factors.py` pins the exact upstream URI it verifies against,
  including the CF table version — never a `current/` alias, which would make
  the provenance recorded here unreproducible.
* Bumping a vocabulary to a new upstream version means updating the version and
  retrieval date here and in the `.bvnr` header in the same commit.
* Bringing in anything this project did not author — a font, a library, an
  image, an algorithm adapted from a published implementation, a document
  adapted from someone else's — means an entry in Part 2 in the same commit,
  and, where the licence requires the text itself to travel with the file, a
  licence file beside it. `CONTRIBUTING.md` asks this as a review question,
  because no build gate can: a licence check would have to know where a string
  came from.
* Every claim in this file is meant to be checkable from the repository. Where
  something is not — where it would depend on how the author worked rather than
  on what the tree contains — this file says so instead of asserting it. The
  UN/ECE entry is the worked example.
* **`§` here means a section of a Bovnar document; another document's section is
  written out as "section N".** The distinction is not decoration. This file
  cites six other people's licences far more often than it cites its own project,
  and `check_doc_refs.py` — which resolves a bare `§N` against `doc/`, and would
  read "UCUM section 3" as a citation into the Bovnar specification — does not
  scan the repository root at all, so nothing here would catch the confusion.
* **The links in this file are not gated.** `check_web_links.py` resolves
  internal links only, by design: no test here may need the network. So an
  upstream project that renames its licence file leaves a dead link in a notices
  document, which is the one kind of rot this file cannot detect about itself —
  two of them were already introduced and caught by hand. Spot-check the URLs
  when touching an entry.
* Where the notices must reach a consumer who never sees this repository, that
  path is verified rather than assumed: the Python wheel carries `LICENSE` and
  this file under `dist-info/licenses/` with both declared as `License-File` in
  its metadata, `pack_artifacts.cmake` copies it into every binary archive and
  the amalgamation drop, and `dist/wasm/README.md` — the README npm publishes —
  points at it.
