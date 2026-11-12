# Translation glossary

Rules for every language under `ui/src/locales/<language>/`. They keep the 23 translations consistent with English and with each other. Anyone changing a translation follows them; a reviewer who disagrees with a rule changes it here first, then applies it everywhere.

## Register and tone

- Speak to the user informally, as English does ("you", not "one" or a formal pronoun): German "du", French "tu", Spanish "tú", Italian "tu", Dutch "je", Polish "ty", Hungarian "te", and the informal form in every other language. Use it in every file of the language, never mixed.
- Short and plain, like the English text. Buttons, tabs, menu entries and badges use the shortest natural wording; a button is a verb ("Join", "Save"), not a sentence.
- Use the words that the language's own large apps use for the same thing (the localisations of Windows, Android, Firefox, Discord). No words from a neighbouring language.
- Keep capitalisation rules of the language (e.g. sentence case in French and Spanish titles, nouns capitalised in German).

## Words that stay in English

These are names or established technical terms. They are never translated, transliterated or declined into a different spelling. Where the grammar of a language needs an ending (Finnish, Hungarian, Irish, Maltese …), attach it the way the language does for foreign names (e.g. "Mumcordiin", "Mumcordot", "Mumcord-ilta").

| Term                                                     | Note                                                                                                                                                                                                          |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Mumcord                                                  | Product name.                                                                                                                                                                                                 |
| Mumble                                                   | Protocol and the other client; also in "Mumble server".                                                                                                                                                       |
| mumpeer, mumbot                                          | Companion service names, lower case.                                                                                                                                                                          |
| YouTube, Spotify, Twitch, GitHub, Discord, TeamSpeak     | Service names.                                                                                                                                                                                                |
| Push-to-talk                                             | Keep the English term, with the hyphens. A language may decline or compound it, and capitalise it by its own rules (German "Push-to-Talk-Taste"), but not translate it. The abbreviation "PTT" stays as well. |
| Opus, TLS, QUIC, P2P, SHA-256, RNNoise, VAD, Mumble Link | Technical names.                                                                                                                                                                                              |
| GIF, URL, ID, IP, OK                                     | Abbreviations, unless the language has its own established abbreviation (e.g. French "URL" stays; Greek may write "ID").                                                                                      |
| Beta                                                     | The feature badge, unless the language writes it in its own script (Greek "Beta"/"Βήτα", Bulgarian "Бета").                                                                                                   |
| `{{placeholders}}` and `<0>…</0>` tags                   | Copied exactly; only their position in the sentence may move.                                                                                                                                                 |

## Words that are translated

Everything not listed above is translated, including the product words that the first machine pass sometimes left in English: player, playlist, settings, watch party, stage, overlay, whisper, badge, avatar, plugin, codec, ping, live.

- Use the word the language really uses. When the English loanword is the normal word in that language (e.g. "Plugin" in German, "avatar" in many languages, "ping" in gaming), the loanword _is_ the translation: write it in that language's spelling and grammar.
- "Live" (the badge on a screen share or camera) is translated like any other word ("En direct", "En directo", "In diretta", "Na żywo" …). Only languages that use the English word on their own broadcast badges keep "Live" (e.g. German, Dutch, Swedish, Danish).
- One English term gets one translation in the whole language. The term table of each language below lists the choices; follow it, and update it when a term changes.

## Plural forms

Plural keys end in `_one`, `_two`, `_few`, `_many`, `_other` (and `_zero` where the language has it), as the language's CLDR plural rules require. Each form must be grammatical for the numbers it covers: write `{{count}}` in every form, never a fixed "1", because `_one` also covers 21, 31 … in several languages.

## Term tables per language

Each table lists how the core English terms are translated in that language. A term marked "(unsure)" was chosen by the AI review and needs a native speaker's check (see issue #291).

### Български (bg)

Register: informal "ти" everywhere (ти/те/твой, "Използвай", "Избери"); never formal "вие/ви/Използвайте". Buttons: 2nd person singular imperative ("Запази", "Влез", "Инсталирай", "Премести"), not verbal nouns ("Инсталиране", "Присъединяване"); verbal nouns only for headings, permission names and status.

| English                          | Bulgarian                                                        | Note                                                                      |
| -------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------- |
| channel                          | канал                                                            |                                                                           |
| sub-channel                      | подканал                                                         |                                                                           |
| server                           | сървър                                                           |                                                                           |
| server browser                   | списък със сървъри                                               | (unsure)                                                                  |
| screen share                     | споделяне на екрана                                              |                                                                           |
| share your screen                | Сподели екрана си                                                | also as dialog title                                                      |
| camera                           | камера                                                           |                                                                           |
| watch party                      | общо гледане                                                     | (unsure) alt. "съвместно гледане"                                         |
| player                           | плейър                                                           |                                                                           |
| playlist                         | плейлист                                                         |                                                                           |
| plugin                           | добавка                                                          | not "плъгин"; Firefox uses "добавки"                                      |
| stage                            | сцена                                                            |                                                                           |
| whisper                          | шепот (noun), шепни (verb)                                       | not "шепнене"                                                             |
| Push-to-talk                     | Push-to-talk                                                     | stays English; not "Натисни и говори"                                     |
| push to mute                     | Push-to-mute                                                     | (unsure) not in keep-English list                                         |
| voice activity                   | гласова активност                                                |                                                                           |
| mute                             | заглуши / заглушен                                               |                                                                           |
| deafen                           | оглуши / оглушен                                                 | fix "Заглушаване / включване на звука"                                    |
| friend                           | приятел                                                          |                                                                           |
| friend request                   | покана за приятелство                                            |                                                                           |
| direct message / private message | лично съобщение                                                  |                                                                           |
| message                          | съобщение                                                        |                                                                           |
| notification                     | известие                                                         |                                                                           |
| settings                         | настройки                                                        |                                                                           |
| profile                          | профил                                                           |                                                                           |
| avatar                           | аватар                                                           |                                                                           |
| badge                            | значка                                                           | not "етикет"                                                              |
| overlay                          | наслагване                                                       | (unsure)                                                                  |
| join                             | Влез (button), влизане (noun)                                    | not "Присъединяване"                                                      |
| leave                            | Напусни                                                          | "Излез от" only for views (full screen)                                   |
| connect                          | Свържи се / свързан                                              | not bare "Свържи"                                                         |
| disconnect                       | Прекъсни връзката / без връзка                                   | not "Разкачи", "Връзката е прекратена"                                    |
| certificate                      | сертификат                                                       |                                                                           |
| ban                              | забрана / Забрани                                                | not "Баннване", "Бания", "бан"                                            |
| block (user)                     | блокирай / блокиран                                              | distinct from ban                                                         |
| kick                             | Изгони / изгонване                                               | not "Кик", "Изритване"                                                    |
| move                             | Премести                                                         |                                                                           |
| comment                          | коментар                                                         |                                                                           |
| nickname                         | псевдоним                                                        |                                                                           |
| username                         | потребителско име                                                |                                                                           |
| register                         | Регистрирай се / регистриран                                     |                                                                           |
| admin                            | администратор                                                    | "Админ" ok only in tight menu labels                                      |
| permission                       | разрешение                                                       | not "право/права"                                                         |
| invite                           | покана / Покани                                                  |                                                                           |
| tray                             | системна област                                                  | (unsure) not "трея"                                                       |
| pin                              | Закачи / закачен                                                 | not "фиксира"; for messages/items                                         |
| pin (plugin version)             | Задържи версия / задържане на версия                             | "закачи версия" does not fit                                              |
| ping                             | пинг                                                             | fix untranslated "Ping"                                                   |
| reaction                         | реакция                                                          |                                                                           |
| emoji                            | емоджи                                                           |                                                                           |
| file                             | файл                                                             |                                                                           |
| upload                           | качване / Качи                                                   |                                                                           |
| stream                           | поток                                                            |                                                                           |
| viewer                           | зрител                                                           | "Image viewer" = "Преглед на изображения"                                 |
| live                             | на живо                                                          |                                                                           |
| chat                             | чат                                                              |                                                                           |
| mention                          | споменаване                                                      |                                                                           |
| activity                         | активност                                                        |                                                                           |
| follow                           | Следвай / следване                                               |                                                                           |
| repository                       | хранилище                                                        |                                                                           |
| install                          | Инсталирай                                                       |                                                                           |
| update                           | актуализация / Актуализирай                                      | not "обновление", "Обнови"                                                |
| volume                           | сила на звука                                                    | not bare "Звук"                                                           |
| microphone                       | микрофон                                                         |                                                                           |
| speaker                          | високоговорител (device)                                         | "говорещ" for active talker, "приоритетен говорител" for priority speaker |
| headphones                       | слушалки                                                         |                                                                           |
| noise suppression                | шумопотискане                                                    | not "потискане на шума"                                                   |
| echo cancellation                | премахване на ехото                                              | fix wrong "заглушаване на ехото"                                          |
| hotkey                           | бърз клавиш                                                      |                                                                           |
| shortcut                         | клавишна комбинация                                              | not "пряк път" (that means a file shortcut)                               |
| identity                         | самоличност                                                      | not "идентичност" (unsure)                                                |
| relay                            | реле (релето)                                                    | not "релей"                                                               |
| log                              | дневник                                                          | not "лог", "журнал"                                                       |
| crash report                     | доклад                                                           | not "отчет"                                                               |
| online / offline                 | на линия / не е на линия; "без връзка" for the app being offline | not "онлайн", "офлайн"                                                    |
| peer-to-peer                     | P2P                                                              | keep-English list                                                         |
| root channel                     | основен канал                                                    | not "коренов"                                                             |
| self-view (camera)               | собствен образ                                                   | not "гледка", "изглед"                                                    |
| import / export                  | Импортирай / Експортирай                                         | not "Внеси"                                                               |
| refresh                          | Опресни                                                          | distinct from "Актуализирай"                                              |
| jitter buffer                    | Джитър буфер                                                     | (unsure) alt. "Буфер за джитър"                                           |
| Do Not Disturb                   | Не безпокойте                                                    | (unsure) fixed feature name, only formal form allowed                     |
| keychain                         | ключодържател                                                    | Apple term                                                                |

### Čeština (cs)

Register: informal "ty" (tykání) everywhere: "tvůj", "připoj se", "vyber"; never "vy/váš" or "-te" imperatives. Past-tense verbs about other people use "(a)": "pozval(a)". Buttons: infinitive ("Uložit", "Připojit se", "Nainstalovat"), not the imperative.

| English              | Czech                             | Note                                                                                                                           |
| -------------------- | --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| channel              | kanál                             |                                                                                                                                |
| sub-channel          | podkanál                          |                                                                                                                                |
| server               | server                            |                                                                                                                                |
| server browser       | prohlížeč serverů                 |                                                                                                                                |
| screen share         | sdílení obrazovky                 |                                                                                                                                |
| share your screen    | sdílet obrazovku                  | not "sdílet vlastní obrazovku"                                                                                                 |
| camera               | kamera                            |                                                                                                                                |
| watch party          | společné sledování                |                                                                                                                                |
| player               | přehrávač                         |                                                                                                                                |
| playlist             | playlist                          | normal loanword (YouTube, Spotify); not "seznam"                                                                               |
| plugin               | plugin (pluginy, pluginů)         | not "doplněk" (that is add-on) (unsure)                                                                                        |
| stage                | pódium                            | (unsure)                                                                                                                       |
| whisper              | šeptat (verb), šeptání (noun)     | permission name uses "Šeptání"                                                                                                 |
| shout                | volat (verb), volání (noun)       | "Voláš na: {{target}}"; whisper vs shout = "šeptání a volání" (unsure)                                                         |
| Push-to-talk         | Push-to-talk                      | stays English, always capitalised, also mid-sentence; "klávesa Push-to-talk"                                                   |
| voice activity       | hlasová aktivita                  |                                                                                                                                |
| mute                 | ztlumit mikrofon                  | state: "ztlumený mikrofon"                                                                                                     |
| deafen               | ztlumit zvuk                      | state: "ztlumený zvuk"; server deafen = "ztlumit zvuk na serveru", not "ohlušit"                                               |
| friend               | přítel (přátelé)                  |                                                                                                                                |
| friend request       | žádost o přátelství               |                                                                                                                                |
| direct message       | přímá zpráva                      |                                                                                                                                |
| private message      | soukromá zpráva                   |                                                                                                                                |
| message              | zpráva                            |                                                                                                                                |
| notification         | oznámení                          |                                                                                                                                |
| settings             | nastavení                         |                                                                                                                                |
| profile              | profil                            |                                                                                                                                |
| avatar               | avatar                            |                                                                                                                                |
| badge                | odznak                            |                                                                                                                                |
| overlay              | překrytí                          | Discord: "Překrytí ve hře"; not "overlay"/"překryv" (unsure)                                                                   |
| join                 | připojit se                       | permission "Enter" too, not "vstoupit"                                                                                         |
| leave                | opustit                           |                                                                                                                                |
| connect              | připojit                          |                                                                                                                                |
| disconnect           | odpojit                           |                                                                                                                                |
| certificate          | certifikát                        |                                                                                                                                |
| ban                  | zabanovat (noun: ban, bany)       | "zablokovat" is reserved for Block                                                                                             |
| kick                 | vyhodit                           | button "Vyhodit", not "Vyhoď"                                                                                                  |
| move                 | přesunout                         |                                                                                                                                |
| comment              | komentář                          |                                                                                                                                |
| nickname             | přezdívka                         |                                                                                                                                |
| username             | uživatelské jméno                 |                                                                                                                                |
| register             | registrovat                       |                                                                                                                                |
| admin                | správce; menu/area: správa        | not "Admin"                                                                                                                    |
| permission           | oprávnění                         | Mumble permission names in texts use the grid names in quotes: oprávnění „Zabanovat“, not "právo Ban"                          |
| invite               | pozvánka (noun), pozvat (verb)    |                                                                                                                                |
| tray                 | oznamovací oblast                 | Windows term; not "lišta"                                                                                                      |
| pin                  | připnout                          | "ping" stays "ping"                                                                                                            |
| reaction             | reakce                            |                                                                                                                                |
| emoji                | emoji                             |                                                                                                                                |
| file                 | soubor                            |                                                                                                                                |
| upload (file)        | nahrát                            |                                                                                                                                |
| upload (bandwidth)   | odesílání                         | "limit odesílání", not "nahrávání"                                                                                             |
| stream               | stream                            |                                                                                                                                |
| viewer               | divák (diváci)                    | not "sledující"; image viewer = "prohlížeč obrázků"                                                                            |
| live                 | Živě                              | badge                                                                                                                          |
| chat                 | chat                              |                                                                                                                                |
| mention              | zmínka (noun), zmínit (verb)      |                                                                                                                                |
| activity             | aktivita                          |                                                                                                                                |
| follow               | sledovat                          | "follow system" = "podle systému"                                                                                              |
| repository           | repozitář                         |                                                                                                                                |
| install              | nainstalovat (noun: instalace)    |                                                                                                                                |
| update               | aktualizovat (noun: aktualizace)  |                                                                                                                                |
| volume               | hlasitost                         |                                                                                                                                |
| microphone           | mikrofon                          |                                                                                                                                |
| speaker              | reproduktor; person: mluvčí       | priority speaker = "přednostní řečník"                                                                                         |
| headphones           | sluchátka                         |                                                                                                                                |
| noise suppression    | potlačení šumu                    |                                                                                                                                |
| echo cancellation    | potlačení ozvěny                  |                                                                                                                                |
| push to mute         | ztlumení podržením klávesy        | (unsure)                                                                                                                       |
| hotkey               | klávesová zkratka                 |                                                                                                                                |
| shortcut             | zkratka                           |                                                                                                                                |
| theme                | motiv                             |                                                                                                                                |
| dark                 | tmavý                             |                                                                                                                                |
| light                | světlý (theme); slabý (strength)  |                                                                                                                                |
| tip                  | tip                               |                                                                                                                                |
| wizard               | průvodce                          | "průvodce nastavením"                                                                                                          |
| import               | importovat                        |                                                                                                                                |
| link                 | odkaz; channel link: propojení    |                                                                                                                                |
| favourite / favorite | oblíbené                          | "přidat do oblíbených"                                                                                                         |
| packet inspector     | inspektor paketů                  |                                                                                                                                |
| relay                | relay (masculine: relaye, relayi) | indeclinable in nominative/accusative: "přes relay", "nový relay"; bot = "bot relaye" (unsure: alternative "přenosový server") |
| online / offline     | online / offline                  | indeclinable; capitalised only as a standalone label                                                                           |

### Dansk (da)

Register: informal "du/dig/din", short and plain, sentence case. Buttons: plain imperative ("Gem", "Flyt", "Forbind", "Deltag"); -ere verbs always with accent ("Installér", "Opdatér", "Importér", "Registrér", "Kopiér", "Minimér").

| English                       | Danish                                           | Note                                                                                                                                |
| ----------------------------- | ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| channel                       | kanal                                            |                                                                                                                                     |
| sub-channel                   | underkanal                                       |                                                                                                                                     |
| server                        | server (pl. servere)                             |                                                                                                                                     |
| server browser                | serveroversigt                                   | (unsure)                                                                                                                            |
| screen share                  | skærmdeling                                      |                                                                                                                                     |
| share your screen             | del din skærm                                    |                                                                                                                                     |
| camera                        | kamera                                           |                                                                                                                                     |
| watch party                   | fællesvisning                                    | not "Watch party"/"fællesfilm"; (unsure)                                                                                            |
| player                        | afspiller                                        |                                                                                                                                     |
| playlist                      | playliste                                        | as in Spotify da                                                                                                                    |
| plugin                        | plugin (pl. plugins)                             | loanword is normal                                                                                                                  |
| stage                         | scene                                            | (unsure)                                                                                                                            |
| whisper                       | hvisk (verb), hvisken (noun)                     | button "Hvisk"; permission name "Hvisken"                                                                                           |
| Push-to-talk                  | Push-to-talk                                     | stays English; "Push-to-talk-tast"                                                                                                  |
| voice activity                | stemmeaktivitet                                  |                                                                                                                                     |
| mute                          | slå mikrofonen fra; state "mikrofon slået fra"   | not "lydløs"; media/chat mute = "slå lyden fra"                                                                                     |
| deafen                        | slå lyden fra; state "lyd slået fra"             | "Server-døv" → "lyd slået fra af serveren"                                                                                          |
| friend                        | ven (pl. venner)                                 |                                                                                                                                     |
| friend request                | venneanmodning                                   |                                                                                                                                     |
| direct message                | direkte besked                                   |                                                                                                                                     |
| private message               | privat besked                                    |                                                                                                                                     |
| message                       | besked                                           | not "meddelelse"                                                                                                                    |
| notification                  | notifikation                                     |                                                                                                                                     |
| settings                      | indstillinger                                    |                                                                                                                                     |
| profile                       | profil                                           |                                                                                                                                     |
| avatar                        | avatar                                           |                                                                                                                                     |
| badge                         | mærke                                            | not "badge"; "Beta" badge stays                                                                                                     |
| overlay                       | overlay                                          | loanword; "overlay i spillet"                                                                                                       |
| join                          | deltag (button); "gå ind i" for channels         | "Tilslut" not for join                                                                                                              |
| leave                         | forlad                                           |                                                                                                                                     |
| connect                       | forbind; state "forbundet"                       | not "tilsluttet"                                                                                                                    |
| disconnect                    | afbryd; state "afbrudt"                          | not "koblet fra"                                                                                                                    |
| certificate                   | certifikat                                       |                                                                                                                                     |
| ban                           | udeluk; noun "udelukkelse"                       | not "bandlys"/"banne"                                                                                                               |
| kick                          | smid ud                                          | not "spark"/"smide ud"                                                                                                              |
| move                          | flyt                                             |                                                                                                                                     |
| comment                       | kommentar                                        |                                                                                                                                     |
| nickname                      | kaldenavn                                        |                                                                                                                                     |
| username                      | brugernavn                                       |                                                                                                                                     |
| register                      | registrér; state "registreret"                   |                                                                                                                                     |
| admin                         | administrator; short "admin" ok in badges        |                                                                                                                                     |
| permission                    | tilladelse                                       | not "rettighed"                                                                                                                     |
| invite                        | invitér (verb), invitation (noun)                |                                                                                                                                     |
| tray                          | systembakke                                      | "minimér til systembakken"                                                                                                          |
| pin                           | fastgør; state "fastgjort"                       |                                                                                                                                     |
| reaction                      | reaktion                                         |                                                                                                                                     |
| emoji                         | emoji                                            |                                                                                                                                     |
| file                          | fil                                              |                                                                                                                                     |
| upload                        | upload (noun), upload (verb)                     | "uploadgrænse"                                                                                                                      |
| stream                        | stream                                           | loanword normal in da                                                                                                               |
| viewer                        | seer (pl. seere); image viewer "billedfremviser" |                                                                                                                                     |
| live                          | Live                                             | badge stays English (glossary)                                                                                                      |
| chat                          | chat                                             |                                                                                                                                     |
| mention                       | omtale                                           | "Omtaler dig"                                                                                                                       |
| activity                      | aktivitet                                        |                                                                                                                                     |
| follow                        | følg; "følger"                                   |                                                                                                                                     |
| repository                    | arkiv                                            | not "Repository"                                                                                                                    |
| install                       | installér; state "installeret"                   |                                                                                                                                     |
| update                        | opdatér (verb), opdatering (noun)                |                                                                                                                                     |
| volume                        | lydstyrke                                        |                                                                                                                                     |
| microphone                    | mikrofon                                         |                                                                                                                                     |
| speaker                       | højttaler (device); taler (person speaking)      | "prioriteret taler"                                                                                                                 |
| headphones                    | hovedtelefoner                                   |                                                                                                                                     |
| noise suppression             | støjdæmpning                                     | not "støjundertrykkelse"/"støjreduktion"                                                                                            |
| echo cancellation             | ekkoannullering                                  | not "ekkoundertrykkelse"/"ekkokancellering"; (unsure)                                                                               |
| push to mute                  | Push-to-mute                                     | like Push-to-talk; (unsure)                                                                                                         |
| hotkey                        | genvejstast                                      | list title "Genveje" ok                                                                                                             |
| shortcut                      | genvej                                           |                                                                                                                                     |
| theme                         | tema                                             |                                                                                                                                     |
| dark                          | mørkt                                            | theme                                                                                                                               |
| light                         | lyst                                             | theme; "let" only for strength (blur, level)                                                                                        |
| tip                           | tip (pl. tips)                                   |                                                                                                                                     |
| wizard                        | opsætningsguide                                  |                                                                                                                                     |
| import                        | importér                                         |                                                                                                                                     |
| link                          | link                                             | "linked" = "tilknyttet", not "forbundet"                                                                                            |
| favourite / favorite          | favorit (pl. favoritter)                         |                                                                                                                                     |
| unmute                        | slå mikrofonen til                               |                                                                                                                                     |
| mute for me (local)           | slå lyd fra for mig                              |                                                                                                                                     |
| quotation marks               | »…«                                              | all quotes, incl. technical names (»input«, »auth«)                                                                                 |
| voice (mode, channel, server) | tale                                             | "Tale og lyd", "Forlad tale", "taleserver", "Taleindstillinger"; a person's voice = "stemme"; voice activation = "stemmeaktivering" |
| author (plugin)               | udgiver                                          | not "ophavsmand"; "udgivernøgle" for author key                                                                                     |
| relay                         | relæ                                             |                                                                                                                                     |
| codec                         | kodek (pl. kodekker)                             | plural (unsure)                                                                                                                     |
| pin (version)                 | fastlås; state "fastlåst"                        | "fastgør" only for chats/messages/tiles; unpin = "Frigør"                                                                           |
| join (server card)            | forbind                                          | "Deltag" reads oddly for servers; (unsure)                                                                                          |
| link (channels, verb)         | linke                                            | state "tilknyttet"; (unsure)                                                                                                        |

### Deutsch (de)

Register: informal "du" everywhere ("du/dein/dich", lower case), never "Sie/Ihr"; checked, no formal address found. Buttons: infinitive ("Speichern", "Beitreten", "Verbinden"), not imperative.

| English                            | German                             | Note                                                                                                              |
| ---------------------------------- | ---------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| channel                            | Kanal                              | voice channel = Sprachkanal                                                                                       |
| sub-channel                        | Unterkanal                         |                                                                                                                   |
| server                             | Server                             |                                                                                                                   |
| server browser                     | Serverbrowser                      |                                                                                                                   |
| screen share                       | Bildschirmfreigabe                 |                                                                                                                   |
| share your screen                  | Bildschirm freigeben               | verb "freigeben", not "teilen"                                                                                    |
| camera                             | Kamera                             |                                                                                                                   |
| watch party                        | Watch-Party                        | with hyphen, not "Watch Party" (unsure)                                                                           |
| player                             | Player                             | compounds: Videoplayer, Miniplayer                                                                                |
| playlist                           | Playlist                           | as Spotify/YouTube DE                                                                                             |
| plugin                             | Plugin                             |                                                                                                                   |
| stage                              | Bühne                              |                                                                                                                   |
| whisper                            | Flüstern                           |                                                                                                                   |
| Push-to-talk                       | Push-to-Talk                       | German capitalisation, as in most strings; "Push-to-Talk-Taste" (unsure)                                          |
| voice activity                     | Sprachaktivierung                  | the talk mode; "Sprachaktivität" only for the event chip                                                          |
| mute                               | Stummschalten                      | state: stummgeschaltet                                                                                            |
| deafen                             | Taubschalten                       | one word; state: taubgeschaltet; noun Taubschaltung (unsure)                                                      |
| friend                             | Freund                             | plural Freunde                                                                                                    |
| friend request                     | Freundschaftsanfrage               |                                                                                                                   |
| direct message                     | Direktnachricht                    |                                                                                                                   |
| private message                    | Privatnachricht                    | one word, not "Private Nachricht"                                                                                 |
| message                            | Nachricht                          |                                                                                                                   |
| notification                       | Benachrichtigung                   |                                                                                                                   |
| settings                           | Einstellungen                      |                                                                                                                   |
| profile                            | Profil                             |                                                                                                                   |
| avatar                             | Avatar                             |                                                                                                                   |
| badge                              | Abzeichen                          |                                                                                                                   |
| overlay                            | Overlay                            | in-game overlay = In-Game-Overlay (not "Ingame")                                                                  |
| join                               | Beitreten                          |                                                                                                                   |
| leave                              | Verlassen                          |                                                                                                                   |
| connect                            | Verbinden                          |                                                                                                                   |
| disconnect                         | Trennen                            | state: getrennt; activity: "hat die Verbindung getrennt"                                                          |
| certificate                        | Zertifikat                         |                                                                                                                   |
| ban                                | Sperren                            | noun Sperrung, Sperrliste; not "Bannen"                                                                           |
| kick                               | Kicken                             | as Discord DE                                                                                                     |
| move                               | Verschieben                        |                                                                                                                   |
| comment                            | Kommentar                          |                                                                                                                   |
| nickname                           | Spitzname                          |                                                                                                                   |
| username                           | Benutzername                       |                                                                                                                   |
| user                               | Benutzer                           | never "Nutzer"; also "Mumcord-Benutzer", "Blockierte Benutzer"                                                    |
| user panel                         | Benutzerbereich                    | bottom-left area (unsure)                                                                                         |
| register                           | Registrieren                       |                                                                                                                   |
| admin                              | Admin                              | Verwaltung for "administration"                                                                                   |
| permission                         | Berechtigung                       | not "Recht/Rechte" in admin rules tab (unsure)                                                                    |
| invite                             | Einladung                          | verb: einladen                                                                                                    |
| tray                               | Infobereich                        | Windows DE term; "Tray" only in search terms                                                                      |
| pin                                | Anheften                           | state: angeheftet; "Ping" stays Ping                                                                              |
| reaction                           | Reaktion                           |                                                                                                                   |
| emoji                              | Emoji                              |                                                                                                                   |
| file                               | Datei                              |                                                                                                                   |
| file sharing                       | Dateifreigabe                      | not "Dateiaustausch"                                                                                              |
| upload                             | Hochladen                          | noun/bandwidth: Upload (Upload-Limit)                                                                             |
| stream                             | Stream                             |                                                                                                                   |
| viewer                             | Zuschauer                          | image viewer = Bildbetrachter                                                                                     |
| live                               | Live                               |                                                                                                                   |
| chat                               | Chat                               |                                                                                                                   |
| voice (settings area, wizard step) | Sprache                            | "Sprache & Audio" (unsure: clashes with Sprache = language)                                                       |
| voice (connection)                 | Sprachchat                         | "Sprachchat verlassen", "Sprachchat: {{channel}}"                                                                 |
| relay                              | Relay                              | neuter: das Relay                                                                                                 |
| online / offline                   | online / Online, offline / Offline | kept English, as Discord DE                                                                                       |
| mention                            | Erwähnung                          |                                                                                                                   |
| activity                           | Aktivität                          |                                                                                                                   |
| follow                             | Folgen                             |                                                                                                                   |
| repository                         | Quelle                             | plugin repository only (unsure); source-code repo of a library/plugin stays "Repository"; source code = Quellcode |
| install                            | Installieren                       |                                                                                                                   |
| update                             | Update                             | noun; button/verb: Aktualisieren                                                                                  |
| volume                             | Lautstärke                         |                                                                                                                   |
| microphone                         | Mikrofon                           |                                                                                                                   |
| speaker                            | Lautsprecher                       | device; person speaking = Sprecher                                                                                |
| headphones                         | Kopfhörer                          |                                                                                                                   |
| noise suppression                  | Rauschunterdrückung                |                                                                                                                   |
| echo cancellation                  | Echounterdrückung                  |                                                                                                                   |
| push to mute                       | Push-to-Mute                       | (unsure)                                                                                                          |
| hotkey                             | Tastenkürzel                       |                                                                                                                   |
| shortcut                           | Tastenkürzel                       |                                                                                                                   |
| theme                              | Design                             |                                                                                                                   |
| dark                               | Dunkel                             |                                                                                                                   |
| light                              | Hell                               | theme; strength = Leicht/Schwach                                                                                  |
| tip                                | Tipp                               |                                                                                                                   |
| wizard                             | Einrichtungsassistent              |                                                                                                                   |
| import                             | Importieren                        |                                                                                                                   |
| link                               | Link                               | channel link = Verknüpfung (not "Verbindung")                                                                     |
| favourite / favorite               | Favorit                            |                                                                                                                   |
| link channels                      | Verknüpfen                         | admin "Links" tab = Verknüpfungen                                                                                 |
| ellipsis                           | "Importieren…"                     | no space before "…"; only a range like "Alt+1 … Alt+9" keeps spaces                                               |
| quotes                             | „…“                                | German quotes everywhere; apostrophe stays straight ' ("Los geht's")                                              |

### Ελληνικά (el)

Register: informal singular "εσύ" (σου, imperative "Διάλεξε", "Μπες"); never "εσείς"/"σας". Buttons: verbal noun, as in Windows/Android Greek ("Σύνδεση", "Αποθήκευση", "Εγκατάσταση"), not an imperative.

| English              | Greek                      | Note                                                                     |
| -------------------- | -------------------------- | ------------------------------------------------------------------------ |
| channel              | κανάλι                     |                                                                          |
| sub-channel          | υποκανάλι                  |                                                                          |
| server               | διακομιστής                |                                                                          |
| server browser       | λίστα διακομιστών          | (unsure)                                                                 |
| screen share         | κοινή χρήση οθόνης         |                                                                          |
| share your screen    | κοινή χρήση της οθόνης σου | verb in sentences: "μοιράσου την οθόνη σου"                              |
| camera               | κάμερα                     |                                                                          |
| watch party          | κοινή προβολή              |                                                                          |
| player               | πρόγραμμα αναπαραγωγής     |                                                                          |
| playlist             | λίστα αναπαραγωγής         | not just "λίστα"                                                         |
| plugin               | πρόσθετο                   |                                                                          |
| stage                | σκηνή                      |                                                                          |
| whisper              | ψίθυρος                    | verb ψιθυρίζω; not "ψιθύρισμα"                                           |
| shout                | φώναγμα                    | verb φωνάζω; not "φωνή" (= voice) (unsure)                               |
| Push-to-talk         | Push-to-talk               | stays English                                                            |
| voice activity       | ανίχνευση φωνής            | Mumble mode name; not "δραστηριότητα φωνής" (unsure)                     |
| mute                 | σίγαση                     | state: "σε σίγαση"                                                       |
| deafen               | κώφωση                     | state: "σε κώφωση"; not "κλείσιμο ήχου" (unsure)                         |
| friend               | φίλος                      |                                                                          |
| friend request       | αίτημα φιλίας              |                                                                          |
| direct message       | άμεσο μήνυμα               |                                                                          |
| private message      | ιδιωτικό μήνυμα            |                                                                          |
| message              | μήνυμα                     |                                                                          |
| notification         | ειδοποίηση                 |                                                                          |
| settings             | ρυθμίσεις                  |                                                                          |
| profile              | προφίλ                     |                                                                          |
| avatar               | άβαταρ                     | Greek spelling, as Discord; not "avatar"                                 |
| badge                | σήμα                       | not "έμβλημα"                                                            |
| overlay              | επικάλυψη                  | as Discord; not "overlay"                                                |
| join                 | συμμετοχή                  | Mumble "Enter" permission: "είσοδος"                                     |
| leave                | αποχώρηση                  | full screen/large view: "έξοδος"                                         |
| connect              | σύνδεση                    |                                                                          |
| disconnect           | αποσύνδεση                 |                                                                          |
| certificate          | πιστοποιητικό              |                                                                          |
| ban                  | αποκλεισμός                | server ban; plural "αποκλεισμοί", not "αποκλείσεις"                      |
| block (personal)     | μπλοκάρισμα                | "μπλοκαρισμένος", unblock "ξεμπλοκάρισμα"; must differ from ban (unsure) |
| kick                 | αποβολή                    | as Discord; not "απομάκρυνση"/"διώξιμο"                                  |
| move                 | μετακίνηση                 | not "μεταφορά"                                                           |
| comment              | σχόλιο                     |                                                                          |
| nickname             | ψευδώνυμο                  |                                                                          |
| username             | όνομα χρήστη               |                                                                          |
| register             | εγγραφή                    |                                                                          |
| admin                | διαχειριστής               |                                                                          |
| permission           | δικαίωμα                   | also for plugins; not "άδεια"                                            |
| invite               | πρόσκληση                  |                                                                          |
| tray                 | δίσκος συστήματος          | (unsure) Windows says "περιοχή ειδοποιήσεων"                             |
| pin                  | καρφίτσωμα                 | "καρφιτσωμένο"; "Ping" stays Ping                                        |
| reaction             | αντίδραση                  |                                                                          |
| emoji                | emoji                      |                                                                          |
| file                 | αρχείο                     |                                                                          |
| upload               | μεταφόρτωση                | (unsure) "αποστολή" also common for bandwidth                            |
| stream               | ροή                        | Twitch "player…" string still untranslated English                       |
| viewer               | θεατής                     | image viewer: "προβολή εικόνων"                                          |
| live                 | Ζωντανά                    | badge                                                                    |
| chat                 | συνομιλία                  |                                                                          |
| mention              | αναφορά                    |                                                                          |
| activity             | δραστηριότητα              |                                                                          |
| follow               | ακολούθηση                 |                                                                          |
| repository           | αποθετήριο                 |                                                                          |
| install              | εγκατάσταση                |                                                                          |
| update               | ενημέρωση                  |                                                                          |
| volume               | ένταση                     |                                                                          |
| microphone           | μικρόφωνο                  |                                                                          |
| speaker (device)     | ηχεία                      |                                                                          |
| speaker (person)     | ομιλητής                   |                                                                          |
| headphones           | ακουστικά                  |                                                                          |
| noise suppression    | καταστολή θορύβου          |                                                                          |
| echo cancellation    | ακύρωση ηχούς              |                                                                          |
| push to mute         | πάτα για σίγαση            | (unsure)                                                                 |
| hotkey               | συντόμευση                 | same as shortcut                                                         |
| shortcut             | συντόμευση                 |                                                                          |
| theme                | θέμα                       |                                                                          |
| dark                 | σκούρο                     |                                                                          |
| light (theme)        | ανοιχτό                    | not "φωτεινό"                                                            |
| light (strength)     | ελαφρύ                     | agrees with noun                                                         |
| tip                  | συμβουλή                   |                                                                          |
| wizard               | οδηγός ρύθμισης            |                                                                          |
| import               | εισαγωγή                   |                                                                          |
| link                 | σύνδεσμος                  | linked channels: "σύνδεση καναλιών"                                      |
| favourite / favorite | αγαπημένο                  |                                                                          |
| Beta                 | Beta                       | badge stays Latin                                                        |
| relay                | ρελέ                       | neuter, indeclinable; not "αναμεταδότης" or English "relay"              |
| peer-to-peer         | P2P                        | section title "Ομότιμες συνδέσεις P2P" (unsure)                          |
| side panel           | πλαϊνό πάνελ               |                                                                          |
| positional audio     | χωρικός ήχος               | (unsure)                                                                 |
| plugin (genitive)    | πρόσθετου / πρόσθετων      | not "προσθέτου"                                                          |

### Español (es)

Register: informal "tú" everywhere ("Únete", "tu pantalla"), never "usted"/"vosotros"; neutral Spanish, Spain spelling ("vídeo"). Buttons: infinitive ("Unirse", "Guardar", "Silenciar"); hints and instructions use the tú imperative ("Únete a un canal…").

| English              | Spanish                            | Note                                                                                    |
| -------------------- | ---------------------------------- | --------------------------------------------------------------------------------------- |
| channel              | canal                              |                                                                                         |
| sub-channel          | subcanal                           |                                                                                         |
| server               | servidor                           |                                                                                         |
| server browser       | explorador de servidores           |                                                                                         |
| screen share         | pantalla compartida                | verb: compartir pantalla                                                                |
| share your screen    | compartir tu pantalla              |                                                                                         |
| camera               | cámara                             |                                                                                         |
| watch party          | sesión de visionado                | not English "watch party" (unsure)                                                      |
| player               | reproductor                        |                                                                                         |
| playlist             | lista de reproducción              | "lista" when space is short                                                             |
| plugin               | plugin (pl. plugins)               | drop "complemento"                                                                      |
| stage                | escenario                          |                                                                                         |
| whisper              | susurrar / susurro                 |                                                                                         |
| Push-to-talk         | Push-to-talk                       | stays English per glossary; not "pulsar para hablar"                                    |
| voice activity       | actividad de voz                   |                                                                                         |
| mute                 | silenciar / silenciado             |                                                                                         |
| deafen               | ensordecer / ensordecido           | as Discord ES                                                                           |
| friend               | amigo                              |                                                                                         |
| friend request       | solicitud de amistad               |                                                                                         |
| direct message       | mensaje directo                    |                                                                                         |
| private message      | mensaje privado                    |                                                                                         |
| message              | mensaje                            |                                                                                         |
| notification         | notificación                       |                                                                                         |
| settings             | ajustes                            | Windows ES uses "Configuración"; keep "ajustes" (Android/iOS)                           |
| profile              | perfil                             |                                                                                         |
| avatar               | avatar                             |                                                                                         |
| badge                | insignia                           |                                                                                         |
| overlay              | superposición                      | drop "overlay"                                                                          |
| join                 | unirse                             | Mumble permission "Enter" may stay "Entrar"                                             |
| leave                | salir                              |                                                                                         |
| connect              | conectar / conectado               |                                                                                         |
| disconnect           | desconectar / desconectado         |                                                                                         |
| certificate          | certificado                        |                                                                                         |
| ban                  | banear / baneo (pl. baneos)        | as Discord ES; drop "prohibir", "vetar" (unsure)                                        |
| kick                 | expulsar                           |                                                                                         |
| move                 | mover                              |                                                                                         |
| comment              | comentario                         |                                                                                         |
| nickname             | apodo                              |                                                                                         |
| username             | nombre de usuario                  |                                                                                         |
| register             | registrarse / registrado           |                                                                                         |
| admin                | administración / administrador     | "Admin" menu → "Administración"                                                         |
| permission           | permiso                            |                                                                                         |
| invite               | invitar / invitación               |                                                                                         |
| tray                 | bandeja del sistema                | "bandeja" when short                                                                    |
| pin                  | fijar / fijado                     |                                                                                         |
| reaction             | reacción                           |                                                                                         |
| emoji                | emoji                              |                                                                                         |
| file                 | archivo                            |                                                                                         |
| upload               | subir / subida                     |                                                                                         |
| stream               | transmisión                        | not "flujo"                                                                             |
| viewer               | espectador                         | screen-share viewers; image viewer = "visor"                                            |
| live                 | en directo                         | Latin America says "en vivo"; not used                                                  |
| chat                 | chat                               |                                                                                         |
| mention              | mención / mencionar                |                                                                                         |
| activity             | actividad                          |                                                                                         |
| follow               | seguir                             |                                                                                         |
| repository           | repositorio                        |                                                                                         |
| install              | instalar                           |                                                                                         |
| update               | actualizar / actualización         |                                                                                         |
| volume               | volumen                            |                                                                                         |
| microphone           | micrófono                          |                                                                                         |
| speaker              | altavoz (device) / orador (person) | Latin America: "parlante"/"bocina"                                                      |
| headphones           | auriculares                        | Latin America often "audífonos"                                                         |
| noise suppression    | supresión de ruido                 |                                                                                         |
| echo cancellation    | cancelación de eco                 |                                                                                         |
| push to mute         | pulsar para silenciar              | (unsure)                                                                                |
| hotkey               | atajo                              |                                                                                         |
| shortcut             | atajo (de teclado)                 |                                                                                         |
| theme                | tema                               |                                                                                         |
| dark                 | oscuro                             |                                                                                         |
| light                | claro                              | theme sense; "ligero/suave" for strength levels                                         |
| tip                  | consejo                            |                                                                                         |
| wizard               | asistente                          |                                                                                         |
| import               | importar                           |                                                                                         |
| link                 | enlace / enlazar                   |                                                                                         |
| favourite / favorite | favorito                           |                                                                                         |
| sign in / log in     | iniciar sesión                     | recurring, keep consistent                                                              |
| reset                | restablecer                        | recurring, keep consistent                                                              |
| relay                | relé                               | mumpeer relay; "servidor de retransmisión" or English "relay" are alternatives (unsure) |
| computer             | ordenador                          | Spain usage; not "equipo" (which also means "team")                                     |
| Root channel         | canal raíz                         | not "canal Root"                                                                        |
| peer-to-peer         | peer-to-peer                       | English loanword; "P2P" when short (unsure)                                             |
| quotes               | «…»                                | Spanish angle quotes in UI text, no straight quotes                                     |

### Eesti (et)

Register: informal "sina" (sa/sinu, verbs in 2nd person singular), never "teie". Buttons: 2nd person singular imperative, as short as possible ("Salvesta", "Liitu", "Ühendu"); permission names may stay in the da-infinitive ("Siseneda", "Keelata").

| English              | Estonian                     | Note                                                                               |
| -------------------- | ---------------------------- | ---------------------------------------------------------------------------------- |
| channel              | kanal                        |                                                                                    |
| sub-channel          | alamkanal                    |                                                                                    |
| server               | server                       |                                                                                    |
| server browser       | serverisirvija               | (unsure)                                                                           |
| screen share         | ekraani jagamine             | compound "ekraanijagamise" OK in genitive attributes                               |
| share your screen    | jaga oma ekraani             |                                                                                    |
| camera               | kaamera                      |                                                                                    |
| watch party          | ühisvaatamine                |                                                                                    |
| player               | pleier                       | media player; not "mängija" (= game player) or "meelerdur"                         |
| playlist             | esitusloend                  |                                                                                    |
| plugin               | plugin                       | pl. "pluginad"; not "pistikprogramm"                                               |
| stage                | lava                         | (unsure)                                                                           |
| whisper              | sosista / sosistamine        | verb / noun                                                                        |
| Push-to-talk         | Push-to-talk                 | stays English per glossary; not "Vajuta ja räägi"; "Push-to-talk-klahv"            |
| voice activity       | hääleaktiivsus               | not "hääletegevus"                                                                 |
| mute                 | vaigista / vaigistatud       | not "summuta"                                                                      |
| deafen               | kurdista / kurdistatud       | "Kurdista / lõpeta kurdistamine"; not "summuta" (unsure)                           |
| friend               | sõber                        |                                                                                    |
| friend request       | sõbrakutse                   |                                                                                    |
| direct message       | privaatsõnum                 | same as private message                                                            |
| private message      | privaatsõnum                 |                                                                                    |
| message              | sõnum                        |                                                                                    |
| notification         | teavitus                     |                                                                                    |
| settings             | seaded                       |                                                                                    |
| profile              | profiil                      |                                                                                    |
| avatar               | avatar                       |                                                                                    |
| badge                | märk                         |                                                                                    |
| overlay              | kattekiht                    | not "ülekate"                                                                      |
| join                 | liitu                        | "Siseneda" only for the Enter permission                                           |
| leave                | lahku                        | "välju" for views/full screen                                                      |
| connect              | ühendu                       | intransitive; not "Ühenda" on connect buttons; state "ühendatud"                   |
| disconnect           | katkesta ühendus             | state "ühendus katkenud"                                                           |
| certificate          | sertifikaat                  |                                                                                    |
| ban                  | keelusta / keeld             | verb / noun (pl. "keelud")                                                         |
| kick                 | viska välja                  | not "Eemalda" (clashes with remove) (unsure)                                       |
| move                 | teisalda                     | not "liiguta"                                                                      |
| comment              | kommentaar                   |                                                                                    |
| nickname             | hüüdnimi                     |                                                                                    |
| username             | kasutajanimi                 |                                                                                    |
| register             | registreeru / registreeritud | "registreeri" when an admin registers someone                                      |
| admin                | admin / haldus               | "admin" as role, "haldus" for administration                                       |
| permission           | õigus                        | pl. "õigused"; not "load"                                                          |
| invite               | kutse / kutsu                | noun / verb                                                                        |
| tray                 | salv                         | "süsteemisalv" in full sentences                                                   |
| pin                  | kinnita / kinnitatud         |                                                                                    |
| reaction             | reaktsioon                   |                                                                                    |
| emoji                | emoji                        |                                                                                    |
| file                 | fail                         |                                                                                    |
| upload               | üleslaadimine / laadi üles   |                                                                                    |
| stream               | voog                         | fix typo "Voja" → "Voo"                                                            |
| viewer               | vaataja                      | "pildivaatur" for the image viewer tool                                            |
| live                 | otse                         | broadcast badge "OTSE" style                                                       |
| chat                 | vestlus                      |                                                                                    |
| mention              | mainimine / maini            |                                                                                    |
| activity             | tegevus                      |                                                                                    |
| follow               | jälgi                        | "järgi süsteemi" for theme/language                                                |
| repository           | hoidla                       |                                                                                    |
| install              | paigalda                     | not "installi"                                                                     |
| update               | uuendus / uuenda             | not "värskendus/värskenda" (keep "värskenda" only for refresh)                     |
| volume               | helitugevus                  |                                                                                    |
| microphone           | mikrofon                     |                                                                                    |
| speaker              | kõlar                        | device; "kõneleja" for a person speaking, "eelisrääkija" for priority speaker      |
| headphones           | kõrvaklapid                  |                                                                                    |
| noise suppression    | mürasummutus                 | not "müravaigistus", "müravähendus"                                                |
| echo cancellation    | kajasummutus                 | not "kaja summutamine"                                                             |
| push to mute         | vajuta vaigistamiseks        | (unsure)                                                                           |
| hotkey               | kiirklahv                    |                                                                                    |
| shortcut             | kiirklahv                    |                                                                                    |
| theme                | teema                        |                                                                                    |
| dark                 | tume                         |                                                                                    |
| light                | hele                         | theme; "nõrk" for blur/noise strength                                              |
| tip                  | näpunäide                    |                                                                                    |
| wizard               | seadistusviisard             |                                                                                    |
| import               | impordi / importimine        |                                                                                    |
| link                 | link / lingi                 | channel links: "lingid", not "seosed"                                              |
| favourite / favorite | lemmik                       |                                                                                    |
| relay                | releeserver                  | "releebot" for the bot; "releeandmed" for relay data (unsure)                      |
| peer-to-peer         | P2P                          | "P2P-režiim", "P2P-ühendus"; "otseühendus" for direct connections (unsure)         |
| crash report         | kokkujooksmise aruanne       | "krahhiaruanne" is a shorter alternative (unsure)                                  |
| group                | rühm                         | not "grupp"                                                                        |
| codec                | kodek                        | not "koodek"                                                                       |
| cancel               | loobu                        | dialog buttons; "tühista" only for a running task                                  |
| online / offline     | võrgus / pole võrgus         | person status; "võrguühenduseta" for the app without internet; not "sisse logitud" |
| quotes               | „…“                          | straight apostrophe in foreign-name endings: "Mumble'i"                            |

### Suomi (fi)

Register: informal "sinä" (2nd person singular, usually only as the verb ending: "olet", "sinun asetuksesi"); never formal "Te"/"Olette". Buttons: imperative 2nd person singular ("Liity", "Tallenna", "Yhdistä"); permission names and list labels use nouns, not infinitives ("Kuiskaus", not "Kuiskata").

| English              | Finnish                                      | Note                                                                                     |
| -------------------- | -------------------------------------------- | ---------------------------------------------------------------------------------------- |
| channel              | kanava                                       | "kanavalle/kanavalla"                                                                    |
| sub-channel          | alikanava                                    |                                                                                          |
| server               | palvelin                                     |                                                                                          |
| server browser       | palvelinselain                               |                                                                                          |
| screen share         | näytön jako                                  | compound "näytönjako-" in compounds                                                      |
| share your screen    | jaa näyttösi                                 | not "jaa oma näyttösi"                                                                   |
| camera               | kamera                                       |                                                                                          |
| watch party          | yhteiskatselu                                |                                                                                          |
| player               | soitin                                       | "Soittimen säätimet", not "valitsimet"                                                   |
| playlist             | soittolista                                  |                                                                                          |
| plugin               | lisäosa                                      | not "laajennus"/"liitännäinen"                                                           |
| stage                | lava                                         | (unsure)                                                                                 |
| whisper              | kuiskaus (noun) / kuiskaa (verb)             | permission name = "Kuiskaus"                                                             |
| Push-to-talk         | Push-to-talk                                 | stays English per glossary; not "Puhu painamalla"; "Push-to-talk-näppäin"                |
| voice activity       | puheaktivointi                               | VAD mode; "puheaktiivisuus" only for events (unsure)                                     |
| mute                 | mykistä / mykistetty                         |                                                                                          |
| deafen               | hiljennä kuulokkeet / kuulokkeet hiljennetty | not "mykistä kuulokkeet", "kuurouta", "hiljennä kuuntelu" (unsure)                       |
| friend               | kaveri                                       |                                                                                          |
| friend request       | kaveripyyntö                                 |                                                                                          |
| direct message       | yksityisviesti                               | same as private message                                                                  |
| private message      | yksityisviesti                               |                                                                                          |
| message              | viesti                                       |                                                                                          |
| notification         | ilmoitus                                     |                                                                                          |
| settings             | asetukset                                    |                                                                                          |
| profile              | profiili                                     |                                                                                          |
| avatar               | profiilikuva                                 | not "avatar"                                                                             |
| badge                | merkki                                       |                                                                                          |
| overlay              | peittokuva                                   | "pelinsisäinen peittokuva"; not "pelinaikainen"                                          |
| join                 | liity                                        |                                                                                          |
| leave                | poistu                                       |                                                                                          |
| connect              | yhdistä / yhdistetty                         |                                                                                          |
| disconnect           | katkaise yhteys / yhteys katkaistu           |                                                                                          |
| certificate          | varmenne                                     | fix typo "varennetta"                                                                    |
| ban                  | anna porttikielto / porttikielto             | as Discord fi; "Estä" is reserved for block; not "bannata"                               |
| kick                 | potkaise                                     | permission name "Potkaisu"                                                               |
| move                 | siirrä                                       |                                                                                          |
| comment              | kommentti                                    |                                                                                          |
| nickname             | lempinimi                                    |                                                                                          |
| username             | käyttäjänimi                                 |                                                                                          |
| register             | rekisteröidy / rekisteröitynyt               |                                                                                          |
| admin                | ylläpito / ylläpitäjä                        | area = ylläpito, person = ylläpitäjä; not "Admin"                                        |
| permission           | oikeus / oikeudet                            | not "luvat", "käyttöoikeudet"                                                            |
| invite               | kutsu                                        |                                                                                          |
| tray                 | ilmoitusalue                                 | as Windows fi                                                                            |
| pin                  | kiinnitä / kiinnitetty                       |                                                                                          |
| reaction             | reaktio                                      |                                                                                          |
| emoji                | emoji                                        |                                                                                          |
| file                 | tiedosto                                     |                                                                                          |
| upload               | lähetys / lähetä                             |                                                                                          |
| stream               | lähetys                                      | fix "Stripin" → "Lähetyksen tiedot"                                                      |
| viewer               | katsoja                                      | image viewer = "kuvankatselu"                                                            |
| live                 | Suorana                                      | Yle/Areena use "Suora"; not "Livenä" (unsure)                                            |
| chat                 | chat                                         | loanword is the normal word ("chatti" in inflection: "chattiin")                         |
| mention              | maininta / mainitse                          |                                                                                          |
| activity             | toiminta                                     | not "toiminto" (= function); "Toimintahistoria" ok                                       |
| follow               | seuraa                                       |                                                                                          |
| repository           | arkisto                                      | plugin repositories; "lähdekoodivarasto" only for source code licences                   |
| install              | asenna / asennus                             |                                                                                          |
| update               | päivitä / päivitys                           |                                                                                          |
| volume               | äänenvoimakkuus                              |                                                                                          |
| microphone           | mikrofoni                                    |                                                                                          |
| speaker              | kaiutin (device) / puhuja (person)           |                                                                                          |
| headphones           | kuulokkeet                                   |                                                                                          |
| noise suppression    | kohinanvaimennus                             |                                                                                          |
| echo cancellation    | kaiunpoisto                                  |                                                                                          |
| push to mute         | Push-to-mute                                 | (unsure) follows Push-to-talk rule                                                       |
| hotkey               | pikanäppäin                                  |                                                                                          |
| shortcut             | pikanäppäin                                  | same as hotkey                                                                           |
| theme                | teema                                        |                                                                                          |
| dark                 | tumma                                        |                                                                                          |
| light                | vaalea (theme) / kevyt (strength)            |                                                                                          |
| tip                  | vinkki                                       |                                                                                          |
| wizard               | ohjattu käyttöönotto                         | not "asennusvelho"/"käyttöönottoavustaja" (unsure)                                       |
| import               | tuo                                          |                                                                                          |
| link                 | linkki                                       | channel link verb: "linkitä"                                                             |
| favourite / favorite | suosikki                                     |                                                                                          |
| ping                 | ping                                         | gaming loanword; not "Viive" for ping (latency stays "viive")                            |
| block                | estä / estetty / poista esto                 |                                                                                          |
| priority speaker     | etusijapuhuja                                |                                                                                          |
| relay                | välityspalvelin                              | mumpeer relay; bot = "välitysbotti"; short chip "Välitys"; not "rele"/"välitin" (unsure) |
| peer-to-peer         | vertaisyhteys                                | "vertaisyhteydet", "vertaisyhteystila"; not "Peer-to-peer"                               |
| chat request         | keskustelupyyntö                             | also "keskustelusuostumus" for chat consent; "chat" stays for chat itself (unsure)       |
| online               | paikalla                                     | not "online-tilassa"                                                                     |
| deny (permission)    | kiellä / kielletty                           | "Estä" is block only                                                                     |
| revocation list      | mitätöintilista                              | (unsure)                                                                                 |
| quotation marks      | ”…”                                          | Finnish style, both marks U+201D                                                         |

### Français (fr)

Register: informal "tu" everywhere (as Discord FR), never "vous"; sentence case; typographic apostrophe ’ and a non-breaking space U+00A0 (never U+202F) before : ; ? ! » and after «; quotes « … »; ellipsis “…”. Buttons: infinitive ("Rejoindre", "Enregistrer", "Couper le micro"); instructions in running text use the "tu" imperative ("Rejoins un salon vocal…").

| English              | French                          | Note                                                                                                                                                    |
| -------------------- | ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| channel              | salon                           | as Discord FR ("salon vocal"); not "canal/canaux"                                                                                                       |
| sub-channel          | sous-salon                      | not "sous-canal"                                                                                                                                        |
| server               | serveur                         |                                                                                                                                                         |
| server browser       | navigateur de serveurs          | (unsure)                                                                                                                                                |
| screen share         | partage d’écran                 |                                                                                                                                                         |
| share your screen    | partager ton écran              |                                                                                                                                                         |
| camera               | caméra                          |                                                                                                                                                         |
| watch party          | soirée de visionnage            | feminine; not "watch party"/"party" (unsure)                                                                                                            |
| player               | lecteur                         | image viewer = visionneuse                                                                                                                              |
| playlist             | playlist                        | normal French loanword (YouTube/Spotify FR), feminine                                                                                                   |
| plugin               | plugin                          | loanword, masculin; "module" also possible (unsure)                                                                                                     |
| stage                | scène                           | media stage area (unsure)                                                                                                                               |
| whisper              | chuchoter / chuchotement        | verb / noun                                                                                                                                             |
| Push-to-talk         | Push-to-talk                    | stays English; "touche Push-to-talk"                                                                                                                    |
| voice activity       | activité vocale                 |                                                                                                                                                         |
| mute                 | couper le micro / micro coupé   | own mic; others by admin: "rendre muet / muet"; local mute of a user or conversation: "mettre en sourdine / en sourdine"; media player: "couper le son" |
| deafen               | couper le son / son coupé       | admin: "rendre sourd / sourd"; undeafen = "réactiver le son"                                                                                            |
| friend               | ami                             |                                                                                                                                                         |
| friend request       | demande d’ami                   |                                                                                                                                                         |
| direct message       | message privé                   |                                                                                                                                                         |
| private message      | message privé                   |                                                                                                                                                         |
| message              | message                         |                                                                                                                                                         |
| notification         | notification                    |                                                                                                                                                         |
| settings             | paramètres                      | Quick settings = "Paramètres rapides" (Windows FR); not "réglages"                                                                                      |
| profile              | profil                          |                                                                                                                                                         |
| avatar               | avatar                          |                                                                                                                                                         |
| badge                | badge                           |                                                                                                                                                         |
| overlay              | overlay                         | masculin; "overlay en jeu" as Discord/Steam FR                                                                                                          |
| join                 | rejoindre                       |                                                                                                                                                         |
| leave                | quitter                         |                                                                                                                                                         |
| connect              | se connecter / connecté         |                                                                                                                                                         |
| disconnect           | se déconnecter / déconnecté     |                                                                                                                                                         |
| certificate          | certificat                      |                                                                                                                                                         |
| ban                  | bannir / bannissement           |                                                                                                                                                         |
| kick                 | expulser                        |                                                                                                                                                         |
| move                 | déplacer                        |                                                                                                                                                         |
| comment              | commentaire                     |                                                                                                                                                         |
| nickname             | pseudo                          |                                                                                                                                                         |
| username             | nom d’utilisateur               |                                                                                                                                                         |
| register             | s’enregistrer / enregistré      |                                                                                                                                                         |
| admin                | administration / administrateur | menu label "Administration", role "administrateur"; not "Admin"                                                                                         |
| permission           | permission                      | as Discord FR; not "droit" and "autorisation"                                                                                                           |
| invite               | inviter / invitation            |                                                                                                                                                         |
| tray                 | zone de notification            | Windows FR                                                                                                                                              |
| pin                  | épingler / désépingler          |                                                                                                                                                         |
| ping                 | ping                            | network latency                                                                                                                                         |
| reaction             | réaction                        |                                                                                                                                                         |
| emoji                | emoji                           |                                                                                                                                                         |
| file                 | fichier                         |                                                                                                                                                         |
| upload               | envoyer / envoi                 | bandwidth: "débit montant"                                                                                                                              |
| stream               | flux                            |                                                                                                                                                         |
| viewer               | spectateur                      |                                                                                                                                                         |
| live                 | en direct                       |                                                                                                                                                         |
| chat                 | chat                            | as Twitch FR; not "discussion" in labels (unsure)                                                                                                       |
| mention              | mention / mentionner            |                                                                                                                                                         |
| activity             | activité                        |                                                                                                                                                         |
| follow               | suivre                          | Following = "Suivi"                                                                                                                                     |
| repository           | dépôt                           |                                                                                                                                                         |
| install              | installer / installation        |                                                                                                                                                         |
| update               | mettre à jour / mise à jour     |                                                                                                                                                         |
| volume               | volume                          |                                                                                                                                                         |
| microphone           | microphone                      | "micro" in short labels                                                                                                                                 |
| speaker              | haut-parleurs                   | audio device; person talking = "orateur" (priority speaker = "orateur prioritaire")                                                                     |
| headphones           | casque                          |                                                                                                                                                         |
| noise suppression    | suppression du bruit            | not "suppression de bruit"/"réduction de bruit"                                                                                                         |
| echo cancellation    | annulation d’écho               |                                                                                                                                                         |
| push to mute         | appuyer pour couper le micro    | (unsure)                                                                                                                                                |
| hotkey               | raccourci                       |                                                                                                                                                         |
| shortcut             | raccourci                       | "raccourci clavier" where needed                                                                                                                        |
| theme                | thème                           |                                                                                                                                                         |
| dark                 | sombre                          |                                                                                                                                                         |
| light                | clair                           | intensity level = "léger/légère"                                                                                                                        |
| tip                  | astuce                          |                                                                                                                                                         |
| wizard               | assistant de configuration      |                                                                                                                                                         |
| import               | importer                        |                                                                                                                                                         |
| link                 | lien                            |                                                                                                                                                         |
| relay                | relais                          | mumpeer relay; “diffusion par relais” for fan-out                                                                                                       |
| peer-to-peer         | pair à pair                     | no hyphens; “P2P” only in search terms                                                                                                                  |
| online / offline     | en ligne / hors ligne           |                                                                                                                                                         |
| Apps (menu/button)   | Applis                          | “menu Applis”                                                                                                                                           |
| Do Not Disturb       | Ne pas déranger                 | short badge “NPD” is invented (unsure)                                                                                                                  |
| favourite / favorite | favori                          |                                                                                                                                                         |

### Gaeilge (ga)

Register: informal singular "tú" (do, duit, ort), never plural "sibh"/"-igí". Buttons: 2nd person singular imperative (Ceangail, Sábháil, Téigh isteach), states as past participle (ceangailte, balbhaithe).

| English              | Irish                                         | Note                                                                                  |
| -------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------- |
| channel              | cainéal                                       | pl. cainéil                                                                           |
| sub-channel          | fochainéal                                    | pl. fochainéil                                                                        |
| server               | freastalaí                                    | pl. freastalaithe                                                                     |
| server browser       | brabhsálaí freastalaithe                      |                                                                                       |
| screen share         | comhroinnt scáileáin                          |                                                                                       |
| share your screen    | Comhroinn do scáileán                         |                                                                                       |
| camera               | ceamara                                       |                                                                                       |
| watch party          | Breathnú le chéile                            | was "Féachaint le chéile"/"cóisir féachana" (unsure)                                  |
| watch (verb)         | breathnaigh; ag breathnú                      | issue #291: "Stop ag breathnú", not "Stop ag féachaint"                               |
| view (look at)       | Féach ar                                      | e.g. "Féach ar an bpróifíl"                                                           |
| player               | seinnteoir                                    |                                                                                       |
| playlist             | seinnliosta                                   | téarma.ie spelling; drop "seinmliosta"                                                |
| plugin               | breiseán                                      | pl. breiseáin                                                                         |
| stage                | ardán                                         | (unsure)                                                                              |
| whisper              | cogar; ag cogarnach                           | noun/permission "Cogar", not "Cogarnach"                                              |
| Push-to-talk         | Push-to-talk                                  | stays English (glossary); "eochair Push-to-talk"; drop "Brúigh-chun-cainte"           |
| push to mute         | brúigh chun balbhú                            | (unsure)                                                                              |
| voice activity       | gníomhaíocht ghutha                           | lenited genitive                                                                      |
| mute                 | Balbhaigh; balbhaithe                         |                                                                                       |
| deafen               | Bodhraigh; bodhraithe                         | undeafen: Díbhodhraigh                                                                |
| friend               | cara                                          | pl. cairde                                                                            |
| friend request       | iarratas cairdis                              |                                                                                       |
| direct message       | teachtaireacht dhíreach                       |                                                                                       |
| private message      | teachtaireacht phríobháideach                 |                                                                                       |
| message              | teachtaireacht                                |                                                                                       |
| notification         | fógra                                         | pl. fógraí                                                                            |
| settings             | socruithe                                     |                                                                                       |
| profile              | próifíl                                       |                                                                                       |
| avatar               | abhatár                                       |                                                                                       |
| badge                | suaitheantas                                  |                                                                                       |
| overlay              | forleagan                                     |                                                                                       |
| join                 | Téigh isteach                                 | not "Glac páirt"/"Dul isteach"; fix untranslated "Join"                               |
| leave                | Fág                                           |                                                                                       |
| connect              | Ceangail; ceangailte                          | past "cheangail" (fix "d’cheangail")                                                  |
| disconnect           | Dícheangail; dícheangailte                    | drop "dínasc", "dichasc"                                                              |
| certificate          | teastas                                       | drop "Deimhne"                                                                        |
| ban                  | Coisc (verb); cosc (noun)                     | pl. coscanna; drop "Toirmisc"                                                         |
| kick                 | Caith amach                                   | fix typo "Cait amach"; drop "Ciceáil"                                                 |
| move                 | Bog                                           |                                                                                       |
| comment              | trácht                                        | drop "nóta"/"tuairim" for the user comment                                            |
| nickname             | leasainm                                      |                                                                                       |
| username             | ainm úsáideora                                |                                                                                       |
| register             | Cláraigh; cláraithe                           |                                                                                       |
| admin                | riarachán (area); riarthóir (person)          |                                                                                       |
| permission           | cead                                          | pl. ceadanna                                                                          |
| invite               | cuireadh; Tabhair cuireadh                    | pl. cuirí                                                                             |
| tray                 | tráidire an chórais                           | short: tráidire                                                                       |
| pin                  | Pionnáil; pionnáilte                          | drop "greamaithe" (unsure)                                                            |
| reaction             | imoibriú                                      | (unsure)                                                                              |
| emoji                | emoji                                         |                                                                                       |
| file                 | comhad                                        |                                                                                       |
| upload               | uaslódáil                                     |                                                                                       |
| stream               | sruth                                         |                                                                                       |
| viewer               | breathnóir                                    | pl. breathnóirí; counts "{{count}} breathnóir"; image viewer = amharcóir              |
| live                 | Beo                                           | as on TG4 broadcasts                                                                  |
| chat                 | comhrá                                        |                                                                                       |
| mention              | lua; Luaigh                                   | pl. luanna                                                                            |
| activity             | gníomhaíocht                                  |                                                                                       |
| follow               | Lean; ag leanúint                             | unfollow: Stop ag leanúint (matches "Stop ag breathnú"); drop "Scoir de … a leanúint" |
| repository           | stór                                          |                                                                                       |
| install              | Suiteáil; suiteáilte                          |                                                                                       |
| update               | nuashonrú (noun); Nuashonraigh (verb)         |                                                                                       |
| volume               | airde fuaime                                  | drop "Kreath", "Toirt"                                                                |
| microphone           | micreafón                                     |                                                                                       |
| speaker              | callaire (device); cainteoir (person talking) |                                                                                       |
| headphones           | cluasáin                                      | drop "cluaisíní"                                                                      |
| noise suppression    | laghdú torainn                                | drop "cosc/ceapadh/cuimsiú torainn" (unsure)                                          |
| echo cancellation    | cealú macalla                                 |                                                                                       |
| hotkey               | aicearra                                      | same as shortcut                                                                      |
| shortcut             | aicearra                                      |                                                                                       |
| theme                | téama                                         |                                                                                       |
| dark                 | dorcha                                        |                                                                                       |
| light                | geal (theme); éadrom (strength)               | drop "Solas"                                                                          |
| tip                  | leid                                          | pl. leideanna                                                                         |
| wizard               | treoraí                                       | drop "draoi"                                                                          |
| import               | Iompórtáil                                    |                                                                                       |
| link                 | nasc; nasctha                                 |                                                                                       |
| favourite / favorite | ceanán                                        | pl. ceanáin                                                                           |
| preview              | réamhamharc                                   |                                                                                       |
| on / off (state)     | ar siúl / as                                  | not "múchta" for off                                                                  |
| stop                 | Stop                                          |                                                                                       |
| block (user)         | Bac; bactha                                   | unblock: Díbhac; also for technical blocks (UDP bactha); drop "blocáil/blocáilte"     |
| Apps (button)        | Aipeanna                                      | Apps menu: Roghchlár na nAipeanna; drop "feidhmchláir"                                |
| identity             | aitheantas                                    | drop "féiniúlacht", "céannacht"                                                       |
| connection           | ceangal                                       | pl. ceangail; "nasc" only for links                                                   |
| unlink               | Dínasc                                        | link/unlink only, not disconnect                                                      |
| dismiss              | Ruaig                                         | (unsure)                                                                              |
| nobody (option)      | Níl aon duine                                 | drop "Duine ar bith" (reads as "anyone") (unsure)                                     |
| low-latency mode     | mód moille ísle                               | drop "mód moill íseal"                                                                |
| peer-to-peer         | piar le piar                                  | (unsure)                                                                              |
| CPU                  | LAP                                           | (unsure)                                                                              |
| system keychain      | eochairfháinne an chórais                     | (unsure)                                                                              |
| extras (features)    | gnéithe breise                                | not "breiseáin" (= plugins)                                                           |
| typography           | ’ and “ ”                                     | curly apostrophe and quotes everywhere; ellipsis "…"                                  |

### Hrvatski (hr)

Register: informal "ti" everywhere (tvoj, možeš, odaberi, koristi); never formal "Vi" (vaš, možete, odaberite, koristite). Buttons: 2nd person singular imperative ("Spremi", "Pridruži se", "Poveži se"); permission names and status labels use verbal nouns ("Šaptanje", "Izbacivanje").

| English                               | Croatian                                               | Note                                                                                                                                                   |
| ------------------------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| channel                               | kanal                                                  |                                                                                                                                                        |
| sub-channel                           | podkanal                                               |                                                                                                                                                        |
| server                                | poslužitelj                                            |                                                                                                                                                        |
| server browser                        | preglednik poslužitelja                                |                                                                                                                                                        |
| screen share                          | dijeljenje zaslona                                     | always "zaslon", never "ekran"                                                                                                                         |
| share your screen                     | Podijeli zaslon                                        |                                                                                                                                                        |
| camera                                | kamera                                                 |                                                                                                                                                        |
| watch party                           | zajedničko gledanje                                    |                                                                                                                                                        |
| player                                | reproduktor                                            | not "player"; "mini reproduktor" (unsure)                                                                                                              |
| playlist                              | popis za reprodukciju                                  | short tab label "Popis" allowed; not "Playlista"                                                                                                       |
| plugin                                | dodatak                                                |                                                                                                                                                        |
| stage                                 | medijsko područje                                      | (unsure)                                                                                                                                               |
| whisper                               | šapat (noun), šaptanje (permission), šapni (button)    |                                                                                                                                                        |
| Push-to-talk                          | Push-to-talk                                           | stays English per glossary; not "Pritisni za govor"; "tipka Push-to-talk"                                                                              |
| voice activity                        | glasovna aktivnost                                     |                                                                                                                                                        |
| mute                                  | Isključi mikrofon (self) / Utišaj (others)             | state: "mikrofon isključen" / "utišan"                                                                                                                 |
| deafen                                | Isključi zvuk                                          | state: "zvuk isključen"; server: "Isključi zvuk na poslužitelju", not "Zagluši"                                                                        |
| friend                                | prijatelj                                              |                                                                                                                                                        |
| friend request                        | zahtjev za prijateljstvo                               |                                                                                                                                                        |
| direct message                        | izravna poruka                                         |                                                                                                                                                        |
| private message                       | privatna poruka                                        |                                                                                                                                                        |
| message                               | poruka                                                 |                                                                                                                                                        |
| notification                          | obavijest                                              |                                                                                                                                                        |
| settings                              | postavke                                               |                                                                                                                                                        |
| profile                               | profil                                                 |                                                                                                                                                        |
| avatar                                | avatar                                                 |                                                                                                                                                        |
| badge                                 | značka                                                 | "Beta" badge: "oznaka Beta" acceptable                                                                                                                 |
| overlay                               | overlay (u igri)                                       | loanword common in Croatian gaming (unsure)                                                                                                            |
| join                                  | Pridruži se                                            | permission name "Ulazak"                                                                                                                               |
| leave                                 | Napusti                                                | "Izađi iz" only for views (full screen, large view)                                                                                                    |
| connect                               | Poveži se                                              | state "povezan"                                                                                                                                        |
| disconnect                            | Prekini vezu                                           | not "Odspoji se"; state "nije povezan"                                                                                                                 |
| certificate                           | certifikat                                             |                                                                                                                                                        |
| ban                                   | Zabrani (verb), zabrana (noun)                         | not "Banaj", "Banovi" → "Zabrane"                                                                                                                      |
| kick                                  | Izbaci                                                 |                                                                                                                                                        |
| move                                  | Premjesti                                              |                                                                                                                                                        |
| comment                               | komentar                                               |                                                                                                                                                        |
| nickname                              | nadimak                                                |                                                                                                                                                        |
| username                              | korisničko ime                                         |                                                                                                                                                        |
| register                              | Registriraj se                                         | state "registriran"                                                                                                                                    |
| admin                                 | administrator / Administracija (menu)                  | not bare "Admin"                                                                                                                                       |
| permission                            | dopuštenje                                             | Android/Windows hr; not "dozvola"                                                                                                                      |
| invite                                | pozivnica (noun), Pozovi (verb)                        |                                                                                                                                                        |
| tray                                  | područje obavijesti                                    | Windows hr term; "programska traka" is the taskbar (unsure)                                                                                            |
| pin                                   | Prikvači                                               | "Ping" stays "ping"                                                                                                                                    |
| reaction                              | reakcija                                               |                                                                                                                                                        |
| emoji                                 | emoji                                                  |                                                                                                                                                        |
| file                                  | datoteka                                               |                                                                                                                                                        |
| upload                                | slanje                                                 | not "upload"; "Ograničenje slanja"                                                                                                                     |
| stream                                | prijenos                                               | fix "struja" (electric current)                                                                                                                        |
| viewer                                | gledatelj                                              | image viewer: "preglednik slika"                                                                                                                       |
| live                                  | uživo                                                  |                                                                                                                                                        |
| chat                                  | chat                                                   | established loanword                                                                                                                                   |
| mention                               | spominjanje                                            |                                                                                                                                                        |
| activity                              | aktivnost                                              |                                                                                                                                                        |
| follow                                | Prati                                                  |                                                                                                                                                        |
| repository                            | repozitorij                                            |                                                                                                                                                        |
| install                               | Instaliraj                                             | noun "instalacija"                                                                                                                                     |
| update                                | Ažuriraj                                               | noun "ažuriranje"                                                                                                                                      |
| volume                                | glasnoća                                               |                                                                                                                                                        |
| microphone                            | mikrofon                                               |                                                                                                                                                        |
| speaker                               | zvučnik (device), govornik (person)                    |                                                                                                                                                        |
| headphones                            | slušalice                                              |                                                                                                                                                        |
| noise suppression                     | potiskivanje šuma                                      | not "suzbijanje šuma", "smanjenje buke"                                                                                                                |
| echo cancellation                     | poništavanje jeke                                      |                                                                                                                                                        |
| push to mute                          | pritisni za isključivanje mikrofona                    | (unsure)                                                                                                                                               |
| hotkey                                | prečac                                                 |                                                                                                                                                        |
| shortcut                              | prečac                                                 | "tipkovni prečac" when needed                                                                                                                          |
| favourite / favorite                  | omiljeno; Dodaj u omiljene                             | not "favorit"                                                                                                                                          |
| options                               | mogućnosti                                             | not "opcije" (Microsoft hr); Steam: "Mogućnosti pokretanja na Steamu" (Steam has no hr UI)                                                             |
| reset                                 | Vrati na zadano (settings) / Poništi (avatar, comment) | not "resetiraj"                                                                                                                                        |
| full screen                           | cijeli zaslon                                          | not "cijeli ekran"                                                                                                                                     |
| past-tense actor (started, invited …) | pokrenuo/la, pozvao/la                                 | "/la" everywhere, also 2nd person ("Prestao/la si", "Jesi li mislio/la") (unsure)                                                                      |
| you are connected / not connected     | Povezan/a si, Nisi povezan/a                           | adjectives about "ti" and {{name}} announcements take "/a"; short status labels about others ("utišan", "Registriran") stay masculine generic (unsure) |
| relay                                 | relej                                                  | "bot releja", "Relej poslužitelja {{server}}"                                                                                                          |
| online / offline                      | na mreži / izvan mreže                                 | label "Izvan mreže"; in sentences "nije na mreži" is fine                                                                                              |
| end-to-end encrypted                  | sveobuhvatno šifrirano                                 | WhatsApp hr wording (unsure)                                                                                                                           |
| transport (UDP/TCP)                   | protokol                                               | "prijenos" is reserved for stream (unsure)                                                                                                             |
| quotation marks                       | „…“                                                    | low-high Croatian quotes everywhere, never "…" or „…”                                                                                                  |
| menu path after "u"                   | u odjeljku Postavke → …                                | keeps the menu name in the nominative                                                                                                                  |

### Magyar (hu)

Register: informal "te" everywhere (tegező imperative: "Válaszd", "Használj"); never "Ön/Önt", formal "-jon/-jen" imperatives or "Kérjük". Typography: quotes „…”, spaced en dash " – ", ellipsis "…". Buttons: noun of action ("Csatlakozás", "Mentés", "Telepítés"), as Windows/Android HU do; prompts/questions use 2nd person singular ("Kirúgod {{name}} felhasználót?").

| English                 | Hungarian           | Note                                                                                  |
| ----------------------- | ------------------- | ------------------------------------------------------------------------------------- |
| channel                 | csatorna            |                                                                                       |
| sub-channel             | alcsatorna          |                                                                                       |
| server                  | szerver             | not "kiszolgáló"                                                                      |
| server browser          | szerverböngésző     |                                                                                       |
| screen share            | képernyőmegosztás   |                                                                                       |
| share your screen       | Képernyő megosztása | button/title                                                                          |
| camera                  | kamera              |                                                                                       |
| watch party             | közös nézés         |                                                                                       |
| player                  | lejátszó            |                                                                                       |
| playlist                | lejátszási lista    |                                                                                       |
| plugin                  | bővítmény           |                                                                                       |
| stage                   | színpad             | (unsure)                                                                              |
| whisper                 | suttogás            | verb "suttog"; not "súgás" (= prompting)                                              |
| Push-to-talk            | Push-to-talk        | stays English; capital only at label start, lowercase mid-sentence ("a push-to-talk") |
| voice activity          | hangaktiválás       | VAD mode                                                                              |
| mute                    | némítás             | state "némítva"                                                                       |
| deafen                  | süketítés           | state "süketítve"; undeafen "süketítés feloldása"                                     |
| friend                  | barát               |                                                                                       |
| friend request          | barátkérelem        |                                                                                       |
| direct message          | közvetlen üzenet    |                                                                                       |
| private message         | privát üzenet       |                                                                                       |
| message                 | üzenet              |                                                                                       |
| notification            | értesítés           |                                                                                       |
| settings                | beállítások         |                                                                                       |
| profile                 | profil              |                                                                                       |
| avatar                  | avatar              | not "profilkép"                                                                       |
| badge                   | jelvény             |                                                                                       |
| overlay                 | overlay             | gamer loanword; "játékon belüli overlay" (unsure)                                     |
| join                    | csatlakozás         | channel entry permission may stay "Belépés"                                           |
| leave                   | kilépés             |                                                                                       |
| connect                 | csatlakozás         | state "csatlakozva"                                                                   |
| disconnect              | lecsatlakozás       | state "lecsatlakozva"; not "lekapcsolódás"                                            |
| certificate             | tanúsítvány         |                                                                                       |
| ban                     | kitiltás            |                                                                                       |
| kick                    | kirúgás             |                                                                                       |
| move                    | áthelyezés          |                                                                                       |
| comment                 | megjegyzés          | not "bemutatkozás"                                                                    |
| nickname                | becenév             |                                                                                       |
| username                | felhasználónév      |                                                                                       |
| register                | regisztráció        | verb "regisztrál"                                                                     |
| admin                   | admin               | "adminisztráció" for the section                                                      |
| permission (plugin)     | engedély            | as Android app permissions                                                            |
| permission (server ACL) | jogosultság         | Mumble channel rights                                                                 |
| invite                  | meghívás            | the thing sent: "meghívó"                                                             |
| tray                    | rendszertálca       | "tálca" alone = taskbar (unsure)                                                      |
| pin                     | kitűzés             | state "kitűzve"                                                                       |
| reaction                | reakció             |                                                                                       |
| emoji                   | emoji               |                                                                                       |
| file                    | fájl                |                                                                                       |
| upload                  | feltöltés           |                                                                                       |
| stream                  | közvetítés          | not "folyam"                                                                          |
| viewer                  | néző                | image viewer: "képnézegető"                                                           |
| live                    | élő                 | badge "ÉLŐ"/"Élő"                                                                     |
| chat                    | csevegés            | not "chat"; compounds "csevegőüzenet", "csevegési előzmények"                         |
| mention                 | említés             |                                                                                       |
| activity                | tevékenység         | not "aktivitás"                                                                       |
| follow                  | követés             |                                                                                       |
| repository              | tároló              |                                                                                       |
| install                 | telepítés           |                                                                                       |
| update                  | frissítés           |                                                                                       |
| volume                  | hangerő             |                                                                                       |
| microphone              | mikrofon            |                                                                                       |
| speaker                 | hangszóró           | person talking: "beszélő"                                                             |
| headphones              | fejhallgató         |                                                                                       |
| noise suppression       | zajszűrés           |                                                                                       |
| echo cancellation       | visszhangszűrés     |                                                                                       |
| push to mute            | Push-to-mute        | kept like Push-to-talk (unsure)                                                       |
| hotkey                  | gyorsbillentyű      |                                                                                       |
| shortcut                | gyorsbillentyű      | "billentyűparancsok" only for the keyboard help list                                  |
| theme                   | téma                |                                                                                       |
| dark                    | sötét               |                                                                                       |
| light                   | világos             | theme; intensity "enyhe"                                                              |
| tip                     | tipp                |                                                                                       |
| wizard                  | varázsló            |                                                                                       |
| import                  | importálás          |                                                                                       |
| audio                   | hang                | adjective "hang-"; avoid "audió"                                                      |
| relay                   | átjátszó            | mumpeer relay; not "továbbító"/"relé" (unsure)                                        |
| identity                | identitás           | client certificate identity (unsure)                                                  |
| link                    | link                | "linkelőnézet" for link preview; not "hivatkozás"                                     |
| Beta                    | Beta                | Latin spelling per glossary, not "Béta" (unsure)                                      |
| data saver              | Adatkímélő          | camera quality option                                                                 |
| fps                     | fps                 | not "kép/s"                                                                           |
| positional audio        | térhatású hang      | (unsure)                                                                              |

### Italiano (it)

Register: informal "tu" everywhere (Sei, Puoi, Il tuo …); no "Lei", and no impersonal "si" where English says "you" (e.g. "si possono disattivare" → "puoi disattivare"). Typography: quotes « », straight apostrophe ', "…" without a space before it. Buttons: imperative 2nd person singular ("Entra", "Salva", "Connetti"); infinitive only for permission names and in confirm-dialog questions ("Reimpostare l'avatar?").

| English                   | Italian                                                  | Note                                                                            |
| ------------------------- | -------------------------------------------------------- | ------------------------------------------------------------------------------- |
| channel                   | canale                                                   |                                                                                 |
| sub-channel               | sottocanale                                              |                                                                                 |
| server                    | server                                                   | invariable plural                                                               |
| server browser            | elenco dei server                                        | (unsure)                                                                        |
| screen share              | condivisione schermo                                     |                                                                                 |
| share your screen         | Condividi lo schermo                                     | not "il tuo schermo"                                                            |
| camera                    | fotocamera                                               | webcam; as Windows, Teams, Meet; Discord says "videocamera" (unsure)            |
| watch party               | watch party (f.)                                         | "la watch party"; "Esci dalla watch party", not "dal party" (unsure)            |
| player                    | lettore                                                  | was mostly "player"; "Mini lettore"                                             |
| playlist                  | playlist (f.)                                            |                                                                                 |
| plugin                    | plugin                                                   | invariable plural                                                               |
| stage                     | palco                                                    | (unsure)                                                                        |
| whisper                   | sussurrare / Sussurra                                    | permission name "Sussurrare"                                                    |
| Push-to-talk              | Push-to-talk                                             | "tasto Push-to-talk"                                                            |
| voice activity            | attività vocale                                          |                                                                                 |
| mute                      | Disattiva microfono (self) · Silenzia (others)           | state: "Microfono disattivato" / "Silenziato"; media players: "Disattiva audio" |
| deafen                    | Disattiva audio                                          | state "Audio disattivato"; "Disattiva audio sul server", not "Assorda"          |
| friend                    | amico                                                    |                                                                                 |
| friend request            | richiesta di amicizia                                    |                                                                                 |
| direct message            | messaggio diretto                                        |                                                                                 |
| private message           | messaggio privato                                        |                                                                                 |
| message                   | messaggio                                                |                                                                                 |
| notification              | notifica                                                 |                                                                                 |
| settings                  | impostazioni                                             |                                                                                 |
| profile                   | profilo                                                  |                                                                                 |
| avatar                    | avatar                                                   |                                                                                 |
| badge                     | badge                                                    | not "Distintivo"                                                                |
| overlay                   | overlay                                                  | "Overlay in gioco", not "nel gioco"                                             |
| join                      | Entra / entrare                                          | not "Unisciti"                                                                  |
| leave                     | Esci (da)                                                | "Leave voice" → "Esci dalla voce" (unsure)                                      |
| connect                   | Connetti / connesso                                      |                                                                                 |
| disconnect                | Disconnetti / disconnesso                                |                                                                                 |
| certificate               | certificato                                              |                                                                                 |
| ban                       | Bandisci / ban (noun)                                    | permission name "Bandire", not "Bannare"                                        |
| kick                      | Espelli / espulsione                                     |                                                                                 |
| move                      | Sposta                                                   |                                                                                 |
| comment                   | commento                                                 |                                                                                 |
| nickname                  | nickname                                                 | as Discord (unsure)                                                             |
| username                  | nome utente                                              |                                                                                 |
| register                  | Registrati (self) / registrato                           |                                                                                 |
| admin                     | amministrazione (area) / amministratore (role)           | not "Admin" menu label                                                          |
| permission                | permesso                                                 | not plugin "autorizzazione"                                                     |
| invite                    | invito / Invita                                          |                                                                                 |
| tray                      | area di notifica                                         |                                                                                 |
| pin                       | Fissa / fissato                                          | unpin "Non fissare più"                                                         |
| reaction                  | reazione                                                 |                                                                                 |
| emoji                     | emoji                                                    |                                                                                 |
| file                      | file                                                     |                                                                                 |
| upload                    | upload (bandwidth) / Carica (action)                     |                                                                                 |
| stream                    | trasmissione                                             | was "flusso" (unsure)                                                           |
| viewer                    | spettatore                                               | image viewer: "visualizzatore"                                                  |
| live                      | In diretta                                               | was "Live"                                                                      |
| chat                      | chat                                                     |                                                                                 |
| mention                   | menzione / menziona                                      |                                                                                 |
| activity                  | attività                                                 |                                                                                 |
| follow                    | Segui / Stai seguendo                                    | not "Seguito" for "Following"                                                   |
| repository                | repository                                               |                                                                                 |
| install                   | Installa                                                 |                                                                                 |
| update                    | Aggiorna / aggiornamento                                 |                                                                                 |
| volume                    | volume                                                   |                                                                                 |
| microphone                | microfono                                                |                                                                                 |
| speaker                   | altoparlanti (device) / oratore (person)                 |                                                                                 |
| headphones                | cuffie                                                   |                                                                                 |
| noise suppression         | soppressione del rumore                                  | not "riduzione del rumore" (as Discord)                                         |
| echo cancellation         | cancellazione dell'eco                                   |                                                                                 |
| push to mute              | Push-to-mute                                             | analogous to Push-to-talk (unsure)                                              |
| hotkey                    | scorciatoia                                              |                                                                                 |
| shortcut                  | scorciatoia (da tastiera)                                |                                                                                 |
| theme                     | tema                                                     |                                                                                 |
| dark                      | scuro                                                    |                                                                                 |
| light                     | chiaro (theme) / leggero (strength)                      |                                                                                 |
| tip                       | suggerimento                                             |                                                                                 |
| wizard                    | procedura guidata                                        |                                                                                 |
| import                    | Importa                                                  |                                                                                 |
| link                      | link (URL) / collegamento (linked channels)              |                                                                                 |
| favourite / favorite      | preferito / preferiti                                    |                                                                                 |
| loading                   | Caricamento…                                             |                                                                                 |
| relay                     | relay (m.)                                               | "il relay", kept in English (unsure)                                            |
| mirror                    | mirror (m.)                                              | kept in English (unsure)                                                        |
| unload (developer plugin) | Rimuovi                                                  | not "Scarica" (= download) (unsure)                                             |
| crash report              | rapporto di arresto anomalo (file) / segnalazione (sent) | (unsure)                                                                        |
| activity row label        | "{{action}}: <thing> ({{name}})"                         | selfName "tu"                                                                   |

### Lietuvių (lt)

Register: informal "tu" (tu, tavo, tave; imperatives naudok, pasirink, spustelėk), never "jūs"/"jūsų"/-kite. Buttons: infinitive verb, as in Windows/Firefox lt ("Prisijungti", "Išsaugoti", "Įdiegti"). Gender: generic masculine, never "(-a)"/"(-usi)" markers; prefer neutral wording when easy. Brand names Mumcord/Mumble bare, without „“; use "programa/programoje Mumcord" when a case is needed. Quotes „…“.

| English                     | Lithuanian                          | Note                                                                                             |
| --------------------------- | ----------------------------------- | ------------------------------------------------------------------------------------------------ |
| channel                     | kanalas                             |                                                                                                  |
| sub-channel                 | pokanalis                           | not "subkanalas"                                                                                 |
| server                      | serveris                            |                                                                                                  |
| server browser              | serverių naršyklė                   |                                                                                                  |
| screen share                | ekrano bendrinimas                  |                                                                                                  |
| share your screen           | bendrinti ekraną                    | button "Bendrinti ekraną"                                                                        |
| camera                      | kamera                              |                                                                                                  |
| watch party                 | bendra peržiūra                     | not "žiūrėjimas kartu", "bendras žiūrėjimas"                                                     |
| player                      | grotuvas                            |                                                                                                  |
| playlist                    | grojaraštis                         |                                                                                                  |
| plugin                      | papildinys                          |                                                                                                  |
| stage                       | scena                               |                                                                                                  |
| whisper                     | šnibždėti (verb), šnibždesys (noun) | permission label noun: "Šnibždėjimas" (like Kalbėjimas); not "šnabždėti"/"šnabždėjimas" (unsure) |
| Push-to-talk                | Push-to-talk                        | stays English per glossary; not "Spausk ir kalbėk"                                               |
| voice activity              | balso aktyvumas                     | not "balso aktyvavimas"                                                                          |
| push to mute                | Push-to-mute                        | mirror of Push-to-talk (unsure)                                                                  |
| mute                        | nutildyti / nutildytas              |                                                                                                  |
| deafen                      | išjungti garsą / garsas išjungtas   | not "apkurtinti"                                                                                 |
| friend                      | draugas                             |                                                                                                  |
| friend request              | draugystės užklausa                 |                                                                                                  |
| direct message              | privati žinutė                      | same as private message                                                                          |
| private message             | privati žinutė                      |                                                                                                  |
| message                     | žinutė                              | chat messages; not "pranešimas"                                                                  |
| notification                | pranešimas                          |                                                                                                  |
| settings                    | nustatymai                          |                                                                                                  |
| profile                     | profilis                            |                                                                                                  |
| avatar                      | avataras                            |                                                                                                  |
| badge                       | ženklelis                           | not "ženkliukas"                                                                                 |
| overlay                     | perdanga                            |                                                                                                  |
| join                        | prisijungti                         |                                                                                                  |
| leave                       | išeiti                              | not "palikti"                                                                                    |
| connect                     | prisijungti                         | state: "prisijungta"                                                                             |
| disconnect                  | atsijungti                          | state: "atsijungta"                                                                              |
| certificate                 | sertifikatas                        |                                                                                                  |
| ban                         | uždrausti / draudimas               | not "užblokuoti" (that is block)                                                                 |
| kick                        | išmesti                             |                                                                                                  |
| move                        | perkelti                            |                                                                                                  |
| comment                     | komentaras                          |                                                                                                  |
| nickname                    | slapyvardis                         |                                                                                                  |
| username                    | naudotojo vardas                    |                                                                                                  |
| register                    | registruotis / užregistruotas       |                                                                                                  |
| admin                       | administratorius                    | no bare "Admin"                                                                                  |
| permission                  | leidimas                            | as Android; also for server ACL instead of "teisė" (unsure)                                      |
| invite                      | kvietimas / pakviesti               |                                                                                                  |
| tray                        | sistemos dėklas                     | short "dėklas"                                                                                   |
| pin                         | prisegti / prisegta                 |                                                                                                  |
| pin a version               | užfiksuoti versiją                  | not "prisegti" (unsure)                                                                          |
| ping                        | ping                                | gaming loanword (unsure)                                                                         |
| reaction                    | reakcija                            |                                                                                                  |
| emoji                       | jaustukas                           |                                                                                                  |
| file                        | failas                              |                                                                                                  |
| upload                      | įkelti / įkėlimas                   | bandwidth: "išsiuntimo sparta" (unsure)                                                          |
| stream                      | srautas                             |                                                                                                  |
| viewer                      | žiūrovas                            | image viewer: "paveikslėlių peržiūra"                                                            |
| live                        | Tiesiogiai                          | as LRT's "TIESIOGIAI" badge                                                                      |
| chat                        | pokalbis                            |                                                                                                  |
| mention                     | paminėjimas / paminėti              |                                                                                                  |
| activity                    | veikla                              |                                                                                                  |
| follow                      | sekti                               |                                                                                                  |
| repository                  | saugykla                            |                                                                                                  |
| install                     | įdiegti                             |                                                                                                  |
| update                      | atnaujinti (verb), naujinys (noun)  | not "naujinti" for button                                                                        |
| volume                      | garsumas                            |                                                                                                  |
| microphone                  | mikrofonas                          |                                                                                                  |
| speaker                     | garsiakalbis                        | talking person: "kalbėtojas"                                                                     |
| headphones                  | ausinės                             |                                                                                                  |
| noise suppression           | triukšmo slopinimas                 |                                                                                                  |
| echo cancellation           | aido slopinimas                     | not "aido pašalinimas"                                                                           |
| hotkey                      | spartusis klavišas                  |                                                                                                  |
| shortcut                    | spartusis klavišas                  |                                                                                                  |
| theme                       | tema                                |                                                                                                  |
| dark                        | tamsi                               |                                                                                                  |
| light                       | šviesi                              | strength levels: "silpnas", not "lengva"                                                         |
| tip                         | patarimas                           |                                                                                                  |
| wizard                      | vediklis                            | not "vedlys"                                                                                     |
| import                      | importuoti                          |                                                                                                  |
| link                        | nuoroda                             | channel link (Mumble): "susieti / susiejimas", not "sąsaja"                                      |
| favourite / favorite        | mėgstamiausias / mėgstamiausi       | not "mėgstamas"                                                                                  |
| user                        | naudotojas                          | not "vartotojas"                                                                                 |
| save                        | išsaugoti                           | current majority; "įrašyti" for audio recording only                                             |
| reset                       | atkurti                             | restore defaults only; not "atstatyti"                                                           |
| reset data / content, clear | išvalyti                            | plugin data, user comments/avatars (unsure)                                                      |
| options                     | parinktys                           |                                                                                                  |
| relay                       | perdavimo mazgas                    | not "retransliatorius" (unsure)                                                                  |
| peer-to-peer                | P2P                                 | no English "peer-to-peer" in text                                                                |
| keychain / keyring          | sistemos raktų saugykla             | not "raktinė", "raktų pakabukas"                                                                 |
| online / offline            | prisijungęs / neprisijungęs         | generic masculine                                                                                |
| play (media)                | leisti                              | not "groti"                                                                                      |

### Latviešu (lv)

Register: informal "tu" (tu, tev, tevi, tavs/tava; instructions in 2nd person singular imperative: "Izvēlies", "Pievienojies"), never "jūs"/"jūsu". Buttons: infinitive, as in Windows/Firefox lv ("Saglabāt", "Pievienoties", "Instalēt").

| English                  | Latvian                                              | Note                                                                                                                                        |
| ------------------------ | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| channel                  | kanāls                                               |                                                                                                                                             |
| sub-channel              | apakškanāls                                          |                                                                                                                                             |
| server                   | serveris                                             |                                                                                                                                             |
| server browser           | serveru pārlūks                                      | (unsure)                                                                                                                                    |
| screen share             | ekrāna kopīgošana                                    |                                                                                                                                             |
| share your screen        | kopīgot ekrānu                                       |                                                                                                                                             |
| camera                   | kamera                                               |                                                                                                                                             |
| watch party              | kopīga skatīšanās                                    |                                                                                                                                             |
| player                   | atskaņotājs                                          |                                                                                                                                             |
| playlist                 | atskaņošanas saraksts                                | not shortened to "saraksts"                                                                                                                 |
| plugin                   | spraudnis                                            | as Firefox lv                                                                                                                               |
| stage                    | skatuve                                              | (unsure)                                                                                                                                    |
| whisper                  | čukstēt; noun čuksts                                 |                                                                                                                                             |
| Push-to-talk             | Push-to-talk                                         | stays English; "Push-to-talk taustiņš"                                                                                                      |
| voice activity           | balss aktivitāte                                     | also in search terms                                                                                                                        |
| mute                     | apklusināt; apklusināts                              | was mixed with "izslēgt mikrofonu/skaņu"                                                                                                    |
| deafen                   | izslēgt skaņu; skaņa izslēgta                        | not "apkurlināt"                                                                                                                            |
| friend                   | draugs                                               |                                                                                                                                             |
| friend request           | drauga pieprasījums                                  | (unsure)                                                                                                                                    |
| direct message           | tiešā ziņa                                           |                                                                                                                                             |
| private message          | privātā ziņa                                         | definite form, not "privāta ziņa"                                                                                                           |
| message                  | ziņa                                                 | not "ziņojums" (sounds like report)                                                                                                         |
| notification             | paziņojums                                           |                                                                                                                                             |
| settings                 | iestatījumi                                          |                                                                                                                                             |
| profile                  | profils                                              |                                                                                                                                             |
| avatar                   | avatars                                              | not "avatārs"                                                                                                                               |
| badge                    | nozīmīte                                             | also "Beta nozīmīte", not "zīme"                                                                                                            |
| overlay                  | pārklājums                                           |                                                                                                                                             |
| join                     | pievienoties                                         |                                                                                                                                             |
| leave                    | pamest                                               | "iziet" only for full screen/view                                                                                                           |
| connect                  | savienoties; savienots                               | button "Savienoties"; in sentences about the user prefer neutral "savienojums ir aktīvs" / "Nav savienojuma" over masculine "esi savienots" |
| disconnect               | atvienoties; atvienots                               |                                                                                                                                             |
| certificate              | sertifikāts                                          |                                                                                                                                             |
| ban                      | liegt piekļuvi; noun liegums; unban "atcelt liegumu" | server ban; kept apart from block (unsure)                                                                                                  |
| block (user)             | bloķēt; bloķēts; atbloķēt                            | local block-user feature only                                                                                                               |
| kick                     | izmest                                               |                                                                                                                                             |
| move                     | pārvietot                                            |                                                                                                                                             |
| comment                  | komentārs                                            |                                                                                                                                             |
| nickname                 | segvārds                                             | (unsure)                                                                                                                                    |
| username                 | lietotājvārds                                        |                                                                                                                                             |
| register                 | reģistrēties; reģistrēts                             | "Reģistrēt sevi" → "Reģistrēties"                                                                                                           |
| admin                    | administrators                                       | "Admin" alone → "Administrators"                                                                                                            |
| permission               | atļauja                                              |                                                                                                                                             |
| invite                   | uzaicināt; uzaicinājums                              | not "ielūgums"                                                                                                                              |
| tray                     | sistēmas paplāte                                     | (unsure)                                                                                                                                    |
| pin                      | piespraust; piesprausts                              |                                                                                                                                             |
| reaction                 | reakcija                                             |                                                                                                                                             |
| emoji                    | emocijzīme                                           | (unsure)                                                                                                                                    |
| file                     | fails                                                |                                                                                                                                             |
| upload                   | augšupielādēt; augšupielāde                          |                                                                                                                                             |
| stream                   | straume                                              | (unsure)                                                                                                                                    |
| viewer                   | skatītājs                                            | "attēlu skatītājs" for image viewer OK                                                                                                      |
| live                     | Tiešraide                                            | badge translated                                                                                                                            |
| chat                     | tērzēšana                                            | not colloquial "čats"                                                                                                                       |
| mention                  | pieminēt; pieminējums                                |                                                                                                                                             |
| activity                 | aktivitāte                                           | "darbību vēsture" OK for history                                                                                                            |
| follow                   | sekot                                                |                                                                                                                                             |
| repository               | krātuve                                              | not "repozitorijs"                                                                                                                          |
| install                  | instalēt                                             |                                                                                                                                             |
| update                   | atjaunināt; atjauninājums                            |                                                                                                                                             |
| volume                   | skaļums                                              |                                                                                                                                             |
| microphone               | mikrofons                                            |                                                                                                                                             |
| speaker                  | skaļrunis (device); runātājs (person)                |                                                                                                                                             |
| headphones               | austiņas                                             |                                                                                                                                             |
| noise suppression        | trokšņu slāpēšana                                    |                                                                                                                                             |
| echo cancellation        | atbalss slāpēšana                                    |                                                                                                                                             |
| push to mute             | Push-to-mute                                         | kept like Push-to-talk (unsure)                                                                                                             |
| hotkey                   | īsinājumtaustiņš                                     |                                                                                                                                             |
| shortcut                 | īsinājumtaustiņš                                     | same as hotkey                                                                                                                              |
| theme                    | motīvs                                               |                                                                                                                                             |
| dark                     | tumšs                                                |                                                                                                                                             |
| light                    | gaišs (theme); vājš (strength)                       |                                                                                                                                             |
| tip                      | padoms                                               |                                                                                                                                             |
| wizard                   | vednis                                               |                                                                                                                                             |
| import                   | importēt                                             |                                                                                                                                             |
| link                     | saite                                                |                                                                                                                                             |
| favourite / favorite     | izlase                                               |                                                                                                                                             |
| ping                     | ping                                                 | loanword, as gaming use                                                                                                                     |
| Beta                     | Beta                                                 |                                                                                                                                             |
| relay                    | relejs                                               | not "retranslators"                                                                                                                         |
| codec                    | kodeks                                               | (unsure)                                                                                                                                    |
| peer-to-peer             | vienādranga (savienojumi)                            | (unsure)                                                                                                                                    |
| storage / secure storage | glabāšana; drošā glabātuve                           | "krātuve" is only repository (unsure)                                                                                                       |
| jitter buffer            | trīces buferis                                       | (unsure)                                                                                                                                    |
| Aaron (mascot)           | Aarons (Aarona, Aaronu)                              | declined like a Latvian name (unsure)                                                                                                       |
| quotation marks          | „…”                                                  | U+201E … U+201D everywhere                                                                                                                  |

### Malti (mt)

Register: informal singular "int" (2nd person singular verbs: "tista'", "għandek", "tiegħek"), never plural/formal "intom"/"Sinjur". Buttons: imperative 2nd person singular, shortest form ("Żid", "Issejvja", "Ingħaqad", "Ikkonnettja").

| English                            | Maltese                                                 | Note                                                         |
| ---------------------------------- | ------------------------------------------------------- | ------------------------------------------------------------ |
| channel                            | kanal (pl. kanali)                                      |                                                              |
| sub-channel                        | sottokanal (pl. sottokanali)                            |                                                              |
| server                             | server (pl. servers)                                    | loanword is the normal word                                  |
| server browser                     | browser tas-servers                                     | (unsure)                                                     |
| screen share                       | qsim tal-iskrin                                         |                                                              |
| share your screen                  | aqsam l-iskrin tiegħek                                  |                                                              |
| screen (sharedScreen, full screen) | skrin; skrin sħiħ                                       | fixes "Scherm", "Scherm sħiħ", "skjer sħiħ"                  |
| camera                             | kamera                                                  |                                                              |
| watch party                        | ħarsa flimkien                                          | not English "watch party" (unsure)                           |
| player                             | plejer                                                  | Maltese spelling of the loanword; not "player" (unsure)      |
| playlist                           | lista tad-daqq                                          | not "playlist"/"lista" (unsure)                              |
| plugin                             | plugin (pl. plugins)                                    | not "plagin" (unsure)                                        |
| stage                              | palk                                                    |                                                              |
| whisper                            | bisbis (noun); ibbisbes (verb)                          | not "tiflisja", "tuħsi" (unsure)                             |
| Push-to-talk                       | Push-to-talk                                            | stays English, "il-push-to-talk"                             |
| voice activity                     | attività tal-vuċi                                       | "vuċi", not "leħen"                                          |
| mute                               | sikket (button); imsikket (state)                       | not "mutja", "mutat", "Muted" (unsure)                       |
| deafen                             | itfi s-smigħ (button); bla smigħ (state)                |                                                              |
| friend                             | ħabib (pl. ħbieb)                                       |                                                              |
| friend request                     | talba ta' ħbiberija                                     |                                                              |
| private / direct message           | messaġġ privat                                          |                                                              |
| message                            | messaġġ (pl. messaġġi)                                  |                                                              |
| notification                       | notifika (pl. notifiki)                                 |                                                              |
| settings                           | issettjar                                               | not English "settings" (unsure)                              |
| profile                            | profil                                                  |                                                              |
| avatar                             | avatar                                                  | loanword is the normal word                                  |
| badge                              | badge                                                   | loanword (unsure)                                            |
| overlay                            | overlay                                                 | loanword, "l-overlay" (unsure)                               |
| join                               | ingħaqad                                                | permission name "Enter": "Dħul"                              |
| leave                              | itlaq                                                   | "ħalli" means "let"; full screen: "oħroġ mill-iskrin sħiħ"   |
| connect                            | ikkonnettja; konness                                    | not "qabbad"                                                 |
| disconnect                         | skonnettja; skonness                                    | not "skonnettjat"                                            |
| certificate                        | ċertifikat                                              |                                                              |
| ban                                | ipprojbixxi (verb); projbizzjoni (pl. projbizzjonijiet) | not "banna", "projbimenti"                                   |
| kick                               | keċċi                                                   | not "kik"                                                    |
| move                               | mexxi                                                   | not "iċċaqlaq"                                               |
| comment                            | kumment (pl. kummenti)                                  | not "kummentarju"                                            |
| nickname                           | laqam                                                   |                                                              |
| username                           | isem tal-utent                                          |                                                              |
| register                           | irreġistra; reġistrat                                   |                                                              |
| admin                              | amministratur; amministrazzjoni                         |                                                              |
| permission                         | permess (pl. permessi)                                  |                                                              |
| invite                             | stieden (verb); stedina (pl. stediniet)                 |                                                              |
| tray                               | żona tan-notifiki                                       | as in Windows; not "tray" (unsure)                           |
| pin                                | iffissa; iffissat                                       | "waħħal" is reserved for paste (unsure)                      |
| paste                              | waħħal; imwaħħal                                        | as in Maltese Windows                                        |
| reaction                           | reazzjoni                                               |                                                              |
| emoji                              | emoji                                                   |                                                              |
| file                               | fajl (pl. fajls)                                        |                                                              |
| upload                             | tella' (verb); upload (noun)                            | (unsure)                                                     |
| stream                             | xandira                                                 | not "fluss" (unsure)                                         |
| viewer                             | spettatur (pl. spettaturi)                              | not "telespettatur"; image viewer: "wiri tal-istampi"        |
| live                               | dirett; "live level": livell fil-ħin reali              | not "Live" (unsure)                                          |
| chat                               | chat ("iċ-chat")                                        | loanword is the normal word                                  |
| mention                            | semma (verb); referenza (noun)                          | (unsure)                                                     |
| activity                           | attività                                                |                                                              |
| follow                             | segwi                                                   |                                                              |
| repository                         | repożitorju                                             |                                                              |
| install                            | installa; installat                                     |                                                              |
| update                             | aġġorna (verb); aġġornament (noun)                      |                                                              |
| volume                             | volum                                                   |                                                              |
| microphone                         | mikrofonu                                               |                                                              |
| speaker (device)                   | spiker (pl. spikers)                                    |                                                              |
| speaker (person talking)           | kelliem                                                 |                                                              |
| headphones                         | headphones                                              | loanword (unsure)                                            |
| noise suppression                  | tnaqqis tal-istorbju                                    | not "tneħħija tal-istorbju"                                  |
| echo cancellation                  | tneħħija tal-eku                                        |                                                              |
| push to mute                       | Push-to-mute                                            | like Push-to-talk (unsure)                                   |
| hotkey / shortcut                  | shortcut (pl. shortcuts)                                | loanword (unsure)                                            |
| add                                | żid                                                     | correct imperative; "iżid" means "he adds" (unsure)          |
| remove                             | neħħi                                                   |                                                              |
| delete                             | ħassar                                                  |                                                              |
| save                               | issejvja                                                | not "salva"                                                  |
| cancel                             | ikkanċella                                              |                                                              |
| close                              | agħlaq                                                  |                                                              |
| key (keyboard)                     | buttuna (pl. buttuni)                                   | same word as button; "ċavetta" only for crypto keys (unsure) |
| key (signing/crypto)               | ċavetta                                                 |                                                              |
| fingerprint (certificate/key)      | marka diġitali                                          | not "fingerprint", "marka tas-swaba'"                        |
| online / offline                   | online / offline                                        | loanword; not "onlajn/offlajn"                               |
| media                              | midja                                                   |                                                              |
| relay                              | relay                                                   | loanword (unsure)                                            |
| preview                            | preview (pl. previews)                                  | loanword (unsure)                                            |
| Typography                         | ' (ASCII apostrophe), “…” quotes, …                     |                                                              |

### Nederlands (nl)

Register: informal "je/jij/jouw" everywhere (never "u/uw"). Buttons: infinitive verb, verb last ("Deelnemen", "Opslaan", "Vriend toevoegen").

| English              | Dutch                  | Note                                                                                  |
| -------------------- | ---------------------- | ------------------------------------------------------------------------------------- |
| channel              | kanaal                 |                                                                                       |
| sub-channel          | subkanaal              |                                                                                       |
| server               | server                 |                                                                                       |
| server browser       | serverbrowser          |                                                                                       |
| screen share         | schermdeling           | verb: "scherm delen"                                                                  |
| share your screen    | je scherm delen        |                                                                                       |
| camera               | camera                 |                                                                                       |
| watch party          | watchparty             | not "kijkfeest" (unsure)                                                              |
| player               | speler                 | "videospeler", "minispeler"; never "player"                                           |
| playlist             | afspeellijst           |                                                                                       |
| plugin               | plug-in                | plural "plug-ins"; not "plugin"                                                       |
| stage                | podium                 | (unsure)                                                                              |
| whisper              | fluisteren             | noun/verb                                                                             |
| Push-to-talk         | Push-to-talk           | key: "push-to-talk-toets"                                                             |
| voice activity       | spraakactivering       | as talk mode; "spraakactiviteit" only for events                                      |
| mute                 | dempen                 | state: "gedempt"                                                                      |
| deafen               | doof zetten            | state: "doof gezet"; toggle "Doof zetten / opheffen" (unsure)                         |
| friend               | vriend                 |                                                                                       |
| friend request       | vriendschapsverzoek    |                                                                                       |
| direct message       | privébericht           | same as private message                                                               |
| private message      | privébericht           |                                                                                       |
| message              | bericht                |                                                                                       |
| notification         | melding                |                                                                                       |
| settings             | instellingen           |                                                                                       |
| profile              | profiel                |                                                                                       |
| avatar               | avatar                 |                                                                                       |
| badge                | badge                  | "de badge"                                                                            |
| overlay              | overlay                | "in-game-overlay"                                                                     |
| join                 | deelnemen              | not "meedoen"/"binnengaan"; Enter permission name "Betreden"                          |
| leave                | verlaten               |                                                                                       |
| connect              | verbinden              | state: "verbonden"                                                                    |
| disconnect           | verbinding verbreken   | state: "niet verbonden"; short button "Verbreken"                                     |
| certificate          | certificaat            |                                                                                       |
| ban                  | verbannen              | noun "ban"; "Ban opheffen" = unban; not "bannen" (Discord nl)                         |
| kick                 | van server verwijderen | menu + permission name "Van server verwijderen"; dialog button "Verwijderen" (unsure) |
| move                 | verplaatsen            |                                                                                       |
| comment              | opmerking              | user comment; not "reactie"                                                           |
| nickname             | bijnaam                |                                                                                       |
| username             | gebruikersnaam         |                                                                                       |
| register             | registreren            |                                                                                       |
| admin                | beheer                 | person: "beheerder"                                                                   |
| permission           | recht                  | plural "rechten"; plugin permissions too, not "toestemmingen/machtigingen"            |
| invite               | uitnodiging            | verb "uitnodigen"                                                                     |
| tray                 | systeemvak             |                                                                                       |
| pin                  | vastzetten             | state "vastgezet"                                                                     |
| reaction             | reactie                |                                                                                       |
| emoji                | emoji                  |                                                                                       |
| file                 | bestand                |                                                                                       |
| upload               | uploaden               | noun "upload"                                                                         |
| stream               | stream                 |                                                                                       |
| viewer               | kijker                 | image viewer: "afbeeldingsviewer"                                                     |
| live                 | Live                   | badge stays English (glossary)                                                        |
| chat                 | chat                   |                                                                                       |
| mention              | vermelding             | verb "vermelden"                                                                      |
| activity             | activiteit             |                                                                                       |
| follow               | volgen                 | state "Gevolgd"                                                                       |
| unfollow             | niet meer volgen       | not "ontvolgen"                                                                       |
| repository           | repository             | (unsure)                                                                              |
| install              | installeren            |                                                                                       |
| update               | bijwerken              | noun "update"; not "updaten"                                                          |
| volume               | volume                 |                                                                                       |
| microphone           | microfoon              |                                                                                       |
| speaker              | luidspreker            | person talking: "spreker"                                                             |
| headphones           | koptelefoon            |                                                                                       |
| noise suppression    | ruisonderdrukking      |                                                                                       |
| echo cancellation    | echo-onderdrukking     |                                                                                       |
| push to mute         | push-to-mute           | (unsure)                                                                              |
| hotkey               | sneltoets              |                                                                                       |
| shortcut             | sneltoets              |                                                                                       |
| theme                | thema                  |                                                                                       |
| dark                 | donker                 |                                                                                       |
| light                | licht                  |                                                                                       |
| tip                  | tip                    |                                                                                       |
| wizard               | installatiewizard      |                                                                                       |
| import               | importeren             |                                                                                       |
| link                 | link                   | channel links: "koppelingen"                                                          |
| favourite / favorite | favoriet               |                                                                                       |
| voice                | spraak                 | "spraakkanaal", "spraakinstellingen"; not "voice(chat)"                               |
| full screen          | volledig scherm        | not "fullscreen"                                                                      |
| skip links           | skiplinks              | a11y jargon (unsure)                                                                  |
| modifier key         | modificatietoets       | (unsure)                                                                              |
| pop out              | apart venster          | not "losmaken" (= unpin) (unsure)                                                     |
| send / sent          | verzenden / verzonden  | not "versturen/verstuurd"                                                             |
| Not now              | Niet nu                | not "Nu niet"                                                                         |
| quotes               | “…”                    | straight apostrophe in plurals: camera's                                              |

### Polski (pl)

Register: informal "ty" (verbs in 2nd person singular: "możesz", "dołącz"); pronouns lower case mid-sentence ("twój", "cię", "tobie"), as Windows/Android/Discord do, never "Pan/Pani"; prefer "Nie możesz …" over impersonal "Nie można …" where English says "you"; gendered forms written "(-a)" ("wyciszony(-a)", "zaprosił(-a)"); 2nd-person past verbs "(-aś)" ("zablokowałeś(-aś)"). Quotes „…”, dash "–", ellipsis "…", menu paths with "→". Buttons: imperative, 2nd person singular ("Zapisz", "Dołącz", "Zainstaluj"), not the infinitive; permission names and steps use the verbal noun ("Banowanie", "Instalacja").

| English              | Polish                                                 | Note                                                                                                         |
| -------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| channel              | kanał                                                  |                                                                                                              |
| sub-channel          | podkanał                                               |                                                                                                              |
| server               | serwer                                                 |                                                                                                              |
| server browser       | przeglądarka serwerów                                  |                                                                                                              |
| screen share         | udostępnianie ekranu                                   |                                                                                                              |
| share your screen    | Udostępnij ekran                                       | drop "swój"                                                                                                  |
| camera               | kamera                                                 |                                                                                                              |
| watch party          | wspólne oglądanie                                      |                                                                                                              |
| player               | odtwarzacz                                             |                                                                                                              |
| playlist             | playlista                                              |                                                                                                              |
| plugin               | wtyczka                                                |                                                                                                              |
| stage                | scena                                                  | (unsure)                                                                                                     |
| whisper              | szept (noun), szeptanie (permission), szepnij (button) |                                                                                                              |
| Push-to-talk         | Push-to-talk                                           | stays English; "klawisz Push-to-talk"                                                                        |
| voice activity       | aktywacja głosem                                       | talk mode; "aktywność głosowa" only for the event                                                            |
| mute                 | wycisz / wyciszony(-a)                                 |                                                                                                              |
| deafen               | ogłusz / ogłuszony(-a)                                 | as Mumble PL; hotkey "Ogłusz / cofnij ogłuszenie" (unsure)                                                   |
| friend               | znajomy / znajomi                                      |                                                                                                              |
| friend request       | zaproszenie do znajomych                               |                                                                                                              |
| direct message       | wiadomość prywatna                                     | same as private message                                                                                      |
| private message      | wiadomość prywatna                                     |                                                                                                              |
| message              | wiadomość                                              |                                                                                                              |
| notification         | powiadomienie                                          |                                                                                                              |
| settings             | ustawienia                                             |                                                                                                              |
| profile              | profil                                                 |                                                                                                              |
| avatar               | awatar                                                 |                                                                                                              |
| badge                | odznaka                                                |                                                                                                              |
| overlay              | nakładka                                               |                                                                                                              |
| join                 | dołącz                                                 |                                                                                                              |
| leave                | opuść                                                  | "Leave voice" → "Opuść kanał głosowy", not "Opuść głos"                                                      |
| connect              | połącz / połączono                                     |                                                                                                              |
| disconnect           | rozłącz / rozłączono                                   |                                                                                                              |
| certificate          | certyfikat                                             |                                                                                                              |
| ban                  | zbanuj / ban / banowanie                               |                                                                                                              |
| kick                 | wyrzuć / wyrzucanie                                    |                                                                                                              |
| move                 | przenieś                                               |                                                                                                              |
| comment              | komentarz                                              |                                                                                                              |
| nickname             | pseudonim                                              |                                                                                                              |
| username             | nazwa użytkownika                                      |                                                                                                              |
| register             | zarejestruj / zarejestrowany(-a)                       |                                                                                                              |
| admin                | administrator                                          | "Admin" only as short menu label                                                                             |
| permission           | uprawnienie                                            |                                                                                                              |
| invite               | zaproszenie / zaproś                                   |                                                                                                              |
| tray                 | zasobnik systemowy                                     |                                                                                                              |
| pin                  | przypnij / przypięte                                   |                                                                                                              |
| ping                 | ping                                                   |                                                                                                              |
| reaction             | reakcja                                                |                                                                                                              |
| emoji                | emoji                                                  |                                                                                                              |
| file                 | plik                                                   |                                                                                                              |
| upload               | wysyłanie / prześlij                                   |                                                                                                              |
| stream               | transmisja                                             | (unsure)                                                                                                     |
| viewer               | widz / widzowie                                        | not "oglądający"                                                                                             |
| live                 | na żywo                                                |                                                                                                              |
| chat                 | czat                                                   |                                                                                                              |
| mention              | wzmianka                                               |                                                                                                              |
| activity             | aktywność                                              |                                                                                                              |
| follow               | obserwuj                                               | not "Śledź"                                                                                                  |
| repository           | repozytorium                                           |                                                                                                              |
| install              | zainstaluj / instalacja                                |                                                                                                              |
| update               | aktualizacja / zaktualizuj                             | button "Zaktualizuj", not "Aktualizuj"                                                                       |
| volume               | głośność                                               |                                                                                                              |
| microphone           | mikrofon                                               |                                                                                                              |
| speaker (device)     | głośniki                                               |                                                                                                              |
| speaker (person)     | mówca                                                  |                                                                                                              |
| headphones           | słuchawki                                              |                                                                                                              |
| noise suppression    | tłumienie szumów                                       | not "redukcja szumów", "odszumianie"                                                                         |
| echo cancellation    | usuwanie echa                                          |                                                                                                              |
| push to mute         | Push-to-mute                                           | stays English like Push-to-talk (unsure)                                                                     |
| hotkey               | skrót klawiszowy                                       |                                                                                                              |
| shortcut             | skrót klawiszowy                                       | short form "skrót"                                                                                           |
| theme                | motyw                                                  |                                                                                                              |
| dark                 | ciemny                                                 |                                                                                                              |
| light (theme)        | jasny                                                  | other senses by context ("lekki", "słaby")                                                                   |
| tip                  | wskazówka                                              |                                                                                                              |
| wizard               | kreator                                                |                                                                                                              |
| import               | importuj / import                                      |                                                                                                              |
| link                 | link                                                   | "links" between channels = "połączenia"                                                                      |
| favourite / favorite | ulubione                                               | "Dodaj do ulubionych"                                                                                        |
| Mumcord              | Mumcord                                                | declined as a masculine noun: Mumcorda, Mumcordowi, Mumcordem, w Mumcordzie; nominative/accusative "Mumcord" |
| preset               | preset / presety                                       | loanword, as in settings; not "ustawienia wstępne" (unsure)                                                  |
| system keychain      | systemowy magazyn kluczy                               | chip "Magazyn kluczy"; not "pęk kluczy", "brelok" (unsure)                                                   |
| secure storage       | bezpieczny magazyn systemu                             |                                                                                                              |
| online / offline     | online / offline                                       | kept as in Polish apps                                                                                       |
| relay                | przekaźnik                                             |                                                                                                              |

### Português (pt)

Register: European Portuguese (pt-PT, post-1990 spelling: ecrã, ficheiro, utilizador, guardar, definições, câmara, partilhar, registar); informal "tu" everywhere (teu/tua, imperative "Entra", "Escolhe"), no "você"/"o seu"/"pode" forms. Buttons: infinitive verb ("Guardar", "Entrar", "Instalar").

| English                     | Portuguese                         | Note                                                                                                       |
| --------------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| channel                     | canal                              |                                                                                                            |
| sub-channel                 | subcanal                           |                                                                                                            |
| server                      | servidor                           |                                                                                                            |
| server browser              | navegador de servidores            | (unsure)                                                                                                   |
| screen share                | partilha de ecrã                   |                                                                                                            |
| share your screen           | partilhar o teu ecrã               | not "o seu"                                                                                                |
| camera                      | câmara                             | pt-PT spelling                                                                                             |
| watch party                 | sessão de visualização             | not English "watch party" (unsure)                                                                         |
| player                      | leitor                             |                                                                                                            |
| playlist                    | lista de reprodução                | short "lista" only in tight tabs                                                                           |
| plugin                      | plugin                             | loanword is the normal word                                                                                |
| stage                       | palco                              |                                                                                                            |
| whisper                     | sussurrar / sussurro               |                                                                                                            |
| Push-to-talk                | Push-to-talk                       | stays English; not "premir para falar"                                                                     |
| voice activity              | atividade de voz                   |                                                                                                            |
| mute                        | silenciar / silenciado             | not "sem microfone"                                                                                        |
| deafen                      | desativar áudio / áudio desativado | not "ensurdecer" (unsure)                                                                                  |
| friend                      | amigo                              |                                                                                                            |
| friend request              | pedido de amizade                  |                                                                                                            |
| direct message              | mensagem direta                    |                                                                                                            |
| private message             | mensagem privada                   |                                                                                                            |
| message                     | mensagem                           |                                                                                                            |
| notification                | notificação                        |                                                                                                            |
| settings                    | definições                         |                                                                                                            |
| profile                     | perfil                             |                                                                                                            |
| avatar                      | avatar                             |                                                                                                            |
| badge                       | distintivo                         | (unsure)                                                                                                   |
| overlay                     | sobreposição                       | not "overlay"                                                                                              |
| join                        | entrar                             | not "participar"                                                                                           |
| leave                       | sair                               |                                                                                                            |
| connect                     | ligar / ligado                     | not "conectar"                                                                                             |
| disconnect                  | desligar / desligado               |                                                                                                            |
| certificate                 | certificado                        |                                                                                                            |
| ban                         | banir / banimento                  |                                                                                                            |
| kick                        | expulsar                           |                                                                                                            |
| move                        | mover                              | not "deslocar"                                                                                             |
| comment                     | comentário                         |                                                                                                            |
| nickname                    | alcunha                            |                                                                                                            |
| username                    | nome de utilizador                 |                                                                                                            |
| register                    | registar / registado               |                                                                                                            |
| admin                       | administração / administrador      | translate the "Admin" menu too                                                                             |
| permission                  | permissão                          |                                                                                                            |
| invite                      | convite / convidar                 |                                                                                                            |
| tray                        | área de notificação                | Windows pt-PT term; not "bandeja"                                                                          |
| pin                         | afixar / afixado                   | pinned messages/channels; not "fixar"                                                                      |
| pin version                 | fixar a versão                     | version pinning only (unsure)                                                                              |
| reaction                    | reação                             |                                                                                                            |
| emoji                       | emoji                              |                                                                                                            |
| file                        | ficheiro                           |                                                                                                            |
| upload                      | envio / enviar                     | not "upload"                                                                                               |
| stream                      | transmissão                        | not "fluxo"                                                                                                |
| viewer                      | espetador                          | image viewer = "visualizador"                                                                              |
| live                        | em direto                          |                                                                                                            |
| chat                        | chat                               |                                                                                                            |
| mention                     | menção / mencionar                 |                                                                                                            |
| activity                    | atividade                          |                                                                                                            |
| follow                      | seguir                             |                                                                                                            |
| repository                  | repositório                        |                                                                                                            |
| install                     | instalar                           |                                                                                                            |
| update                      | atualizar / atualização            |                                                                                                            |
| volume                      | volume                             |                                                                                                            |
| microphone                  | microfone                          |                                                                                                            |
| speaker                     | altifalantes                       | device; a person speaking = "orador"; not "colunas"                                                        |
| headphones                  | auscultadores                      |                                                                                                            |
| noise suppression           | supressão de ruído                 |                                                                                                            |
| echo cancellation           | cancelamento de eco                |                                                                                                            |
| push to mute                | premir para silenciar              |                                                                                                            |
| hotkey                      | atalho                             |                                                                                                            |
| shortcut                    | atalho                             |                                                                                                            |
| theme                       | tema                               |                                                                                                            |
| dark                        | escuro                             |                                                                                                            |
| light                       | claro                              | theme only; intensity = "leve"                                                                             |
| tip                         | dica                               |                                                                                                            |
| wizard                      | assistente                         |                                                                                                            |
| import                      | importar                           |                                                                                                            |
| link                        | ligação                            | Firefox pt-PT; not "link"                                                                                  |
| favourite / favorite        | favorito                           |                                                                                                            |
| password                    | palavra-passe                      |                                                                                                            |
| save                        | guardar                            |                                                                                                            |
| user                        | utilizador                         |                                                                                                            |
| relay                       | retransmissor                      | masculine noun everywhere ("via retransmissor", "o retransmissor"); not "retransmissão" / "relay" (unsure) |
| online / offline (presence) | online / offline                   | loanwords, also in counts ("{{count}} online"); "ligado" only for connections                              |
| encrypted                   | cifrado                            | not "encriptado"                                                                                           |
| quotes                      | « »                                | for UI names and quoted text; no "…" or “…”                                                                |
| Push-to-talk                | (casing)                           | capital only at sentence start; "o push-to-talk" mid-sentence                                              |

### Română (ro)

Register: informal "tu" (2nd person singular: "poți", "îți", "ești"), never "dumneavoastră" or plural "-ți" forms ("Folosiți", "Reporniți"); diacritics ș/ț with comma below. Buttons: imperative 2nd person singular ("Salvează", "Intră", "Mută"), reflexive "-te" when the user themself acts ("Conectează-te", "Deconectează-te", "Înregistrează-te").

| English              | Romanian                                                                                  | Note                                                                                                                            |
| -------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| channel              | canal (pl. canale)                                                                        |                                                                                                                                 |
| sub-channel          | subcanal                                                                                  |                                                                                                                                 |
| server               | server (pl. servere)                                                                      |                                                                                                                                 |
| server browser       | lista de servere                                                                          | (unsure) no strings yet                                                                                                         |
| screen share         | partajare de ecran                                                                        | "partajarea ecranului" in sentences                                                                                             |
| share your screen    | Partajează-ți ecranul                                                                     | not "Partajează ecranul tău"                                                                                                    |
| camera               | cameră                                                                                    |                                                                                                                                 |
| watch party          | vizionare comună                                                                          | "Urmăriți împreună" → "Vizionare comună" / "Urmărește împreună"                                                                 |
| player               | player                                                                                    | loanword, as in Romanian media apps                                                                                             |
| playlist             | playlist                                                                                  | not shortened "Listă"                                                                                                           |
| plugin               | plugin (pl. pluginuri)                                                                    |                                                                                                                                 |
| stage                | scenă                                                                                     | no strings yet                                                                                                                  |
| whisper              | șoaptă (noun), șoptește (verb)                                                            | not "Șoptit"/"Șoptire"                                                                                                          |
| Push-to-talk         | Push-to-talk                                                                              | stays English (glossary)                                                                                                        |
| voice activity       | activitate vocală                                                                         |                                                                                                                                 |
| mute                 | Oprește microfonul / microfon oprit (self); Dezactivează sunetul (others, chats, plugins) | (unsure) not "Fără sunet", "dezactivată"                                                                                        |
| deafen               | Oprește sunetul / sunet oprit                                                             | server: "Oprește sunetul pe server", not "Asurzește" (unsure)                                                                   |
| friend               | prieten                                                                                   |                                                                                                                                 |
| friend request       | cerere de prietenie                                                                       | no strings yet                                                                                                                  |
| direct message       | mesaj direct                                                                              | no strings yet                                                                                                                  |
| private message      | mesaj privat                                                                              |                                                                                                                                 |
| message              | mesaj                                                                                     |                                                                                                                                 |
| notification         | notificare                                                                                |                                                                                                                                 |
| settings             | setări                                                                                    |                                                                                                                                 |
| profile              | profil                                                                                    |                                                                                                                                 |
| avatar               | avatar                                                                                    |                                                                                                                                 |
| badge                | insignă                                                                                   |                                                                                                                                 |
| overlay              | suprapunere                                                                               | (unsure) not "Overlay"/"overlay-ul"                                                                                             |
| join                 | Intră                                                                                     | not "Alătură-te"/"Intra"; permission noun "Intrare"                                                                             |
| leave                | Părăsește                                                                                 | "Ieși din" for views/full screen                                                                                                |
| connect              | Conectează-te / conectat                                                                  |                                                                                                                                 |
| disconnect           | Deconectează-te / deconectat                                                              |                                                                                                                                 |
| certificate          | certificat                                                                                |                                                                                                                                 |
| ban                  | Interzice (verb), interdicție (noun)                                                      | not "Banare"; permission "Interzicere"                                                                                          |
| kick                 | Dă afară (verb), dare afară (permission)                                                  | (unsure) not "Eliminare", "Dat afară"                                                                                           |
| move                 | Mută                                                                                      |                                                                                                                                 |
| comment              | comentariu                                                                                |                                                                                                                                 |
| nickname             | poreclă                                                                                   | no strings yet                                                                                                                  |
| username             | nume de utilizator                                                                        | no strings yet                                                                                                                  |
| register             | Înregistrează-te / înregistrat                                                            | "Auto-înregistrare" for Register self                                                                                           |
| admin                | admin                                                                                     | "administrare" for administration                                                                                               |
| permission           | permisiune                                                                                |                                                                                                                                 |
| invite               | invitație (noun), Invită (verb)                                                           |                                                                                                                                 |
| tray                 | zona de notificare                                                                        | not "tray" (Windows RO term)                                                                                                    |
| pin                  | Fixează / fixat                                                                           |                                                                                                                                 |
| reaction             | reacție                                                                                   | no strings yet                                                                                                                  |
| emoji                | emoji                                                                                     | no strings yet                                                                                                                  |
| file                 | fișier                                                                                    |                                                                                                                                 |
| upload               | încărcare                                                                                 |                                                                                                                                 |
| stream               | transmisiune                                                                              | (unsure) not "flux"                                                                                                             |
| viewer               | spectator                                                                                 | image viewer: "vizualizator"                                                                                                    |
| live                 | În direct                                                                                 | (unsure) Romanian TV often shows "LIVE"                                                                                         |
| chat                 | chat                                                                                      |                                                                                                                                 |
| mention              | mențiune                                                                                  |                                                                                                                                 |
| activity             | activitate                                                                                |                                                                                                                                 |
| follow               | Urmărește                                                                                 |                                                                                                                                 |
| repository           | depozit                                                                                   |                                                                                                                                 |
| keychain             | depozitul de chei al sistemului                                                           | chip label "Depozit de chei"; (unsure) macOS RO says "Portchei"                                                                 |
| relay                | releu (pl. relee)                                                                         | never "relay"/"relay-ul"                                                                                                        |
| end-to-end encrypted | criptat integral                                                                          | as in WhatsApp RO; no "end-to-end" left                                                                                         |
| install              | Instalează / instalare                                                                    |                                                                                                                                 |
| update               | actualizare (noun), Actualizează (verb)                                                   |                                                                                                                                 |
| volume               | volum                                                                                     |                                                                                                                                 |
| microphone           | microfon                                                                                  |                                                                                                                                 |
| speaker              | difuzor (device), vorbitor (person)                                                       | two senses                                                                                                                      |
| headphones           | căști                                                                                     |                                                                                                                                 |
| noise suppression    | suprimarea zgomotului                                                                     | not "eliminarea"                                                                                                                |
| echo cancellation    | anularea ecoului                                                                          |                                                                                                                                 |
| push to mute         | Push-to-mute                                                                              | (unsure) no strings yet                                                                                                         |
| hotkey               | comandă rapidă                                                                            | same as shortcut; not "scurtătură"                                                                                              |
| shortcut             | comandă rapidă                                                                            |                                                                                                                                 |
| theme                | temă                                                                                      |                                                                                                                                 |
| dark                 | întunecat / întunecată                                                                    |                                                                                                                                 |
| light                | luminos / luminoasă (theme)                                                               | "ușor"/"slab" for intensity levels                                                                                              |
| tip                  | sfat                                                                                      |                                                                                                                                 |
| wizard               | asistent de configurare                                                                   |                                                                                                                                 |
| import               | Importă                                                                                   |                                                                                                                                 |
| link                 | link                                                                                      | channel linking (Mumble): "legătură"                                                                                            |
| favourite / favorite | favorit (pl. favorite)                                                                    |                                                                                                                                 |
| off (state)          | oprit / dezactivat                                                                        | "dezactivat" for features, "oprit" for mic/sound                                                                                |
| numbers ≥ 20         | {{count}} de …                                                                            | plural `_other` covers 20+ and takes "de"; `_few` (0, 2–19) never does; non-plural count strings rephrased as "Noun: {{count}}" |

### Slovenčina (sk)

Register: informal "ty" everywhere (tvoj, si, použi); never formal "váš/vaše/Používajte". Buttons: infinitive verb ("Pripojiť sa", "Uložiť", "Presunúť"), as in Windows/Android SK.

Gender: never masculine-only for the user or a named person. Prefer gender-neutral wording ("Nemáš pripojenie", "potom môžeš …", "Používateľ {{name}} je …"); where a past-tense verb is unavoidable, write "(a)" ("si pridal(a)", "{{name}} sa pripojil(a)"). (unsure)

Quotes: „…“. Ellipsis: "…".

| English                               | Slovak                                       | Note                                                                                                          |
| ------------------------------------- | -------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| channel                               | kanál                                        |                                                                                                               |
| sub-channel                           | podkanál                                     |                                                                                                               |
| server                                | server                                       |                                                                                                               |
| server browser                        | prehliadač serverov                          |                                                                                                               |
| screen share                          | zdieľanie obrazovky                          |                                                                                                               |
| share your screen                     | Zdieľať obrazovku                            | drop "vlastnú"                                                                                                |
| camera                                | kamera                                       |                                                                                                               |
| watch party                           | spoločné sledovanie                          |                                                                                                               |
| player                                | prehrávač                                    |                                                                                                               |
| playlist                              | playlist                                     | not "zoznam" alone; YouTube/Spotify SK use "playlist"                                                         |
| plugin                                | doplnok                                      | not "plugin" in text                                                                                          |
| game plugin (Mumble positional audio) | herný doplnok                                | keep "herný" so it differs from Mumcord doplnky (unsure)                                                      |
| stage                                 | pódium                                       | (unsure)                                                                                                      |
| whisper                               | šepkať (verb) / šepkanie (noun)              | not "šepot"                                                                                                   |
| Push-to-talk                          | Push-to-talk                                 | always with hyphens; capital P only at the start of a sentence or label, "push-to-talk" mid-sentence          |
| voice activity                        | hlasová aktivita                             |                                                                                                               |
| mute                                  | stlmiť mikrofón / stlmený mikrofón           | short "Stlmiť" where the mic is obvious                                                                       |
| deafen                                | stlmiť zvuk / stlmený zvuk                   | also "Server deafen" → "Stlmiť zvuk na serveri", not "ohlušiť"                                                |
| friend                                | priateľ                                      |                                                                                                               |
| friend request                        | žiadosť o priateľstvo                        |                                                                                                               |
| direct message                        | súkromná správa                              |                                                                                                               |
| private message                       | súkromná správa                              |                                                                                                               |
| message                               | správa                                       |                                                                                                               |
| notification                          | upozornenie                                  | not "oznámenie" (Android SK)                                                                                  |
| settings                              | nastavenia                                   |                                                                                                               |
| profile                               | profil                                       |                                                                                                               |
| avatar                                | avatar                                       |                                                                                                               |
| badge                                 | odznak                                       |                                                                                                               |
| overlay                               | prekrytie                                    | not "overlay"; Xbox Game Bar SK (unsure)                                                                      |
| join                                  | pripojiť sa                                  | perm "Enter" stays "Vstúpiť"                                                                                  |
| leave                                 | opustiť                                      |                                                                                                               |
| connect                               | pripojiť                                     |                                                                                                               |
| disconnect                            | odpojiť                                      |                                                                                                               |
| certificate                           | certifikát                                   |                                                                                                               |
| ban                                   | zakázať prístup / zákaz                      | short button "Zakázať"; list "Zákazy", not "Bany"/"Zabanovať"; "zablokovať" is for block (unsure)             |
| kick                                  | vyhodiť                                      |                                                                                                               |
| move                                  | presunúť                                     |                                                                                                               |
| comment                               | komentár                                     |                                                                                                               |
| nickname                              | prezývka                                     |                                                                                                               |
| username                              | používateľské meno                           |                                                                                                               |
| register                              | registrovať                                  |                                                                                                               |
| admin                                 | správca (person) / Správa (menu, section)    | not "Admin"                                                                                                   |
| permission                            | oprávnenie                                   | not "povolenie"                                                                                               |
| invite                                | pozvánka / pozvať                            |                                                                                                               |
| tray                                  | systémová lišta                              | short "lišta" (unsure)                                                                                        |
| pin                                   | pripnúť                                      |                                                                                                               |
| reaction                              | reakcia                                      |                                                                                                               |
| emoji                                 | emoji                                        |                                                                                                               |
| file                                  | súbor                                        |                                                                                                               |
| upload                                | odosielanie                                  | not "upload"                                                                                                  |
| stream                                | stream                                       | YouTube SK usage                                                                                              |
| viewer                                | divák                                        | image viewer = "prehliadač"                                                                                   |
| live                                  | Naživo                                       |                                                                                                               |
| chat                                  | chat                                         |                                                                                                               |
| mention                               | zmienka / spomenúť                           |                                                                                                               |
| activity                              | aktivita                                     |                                                                                                               |
| follow                                | sledovať                                     |                                                                                                               |
| repository                            | repozitár                                    |                                                                                                               |
| install                               | nainštalovať                                 | step noun "Inštalácia"                                                                                        |
| update                                | aktualizovať / aktualizácia                  |                                                                                                               |
| volume                                | hlasitosť                                    |                                                                                                               |
| microphone                            | mikrofón                                     |                                                                                                               |
| speaker                               | reproduktor (device) / rečník (person)       |                                                                                                               |
| headphones                            | slúchadlá                                    |                                                                                                               |
| noise suppression                     | potlačenie šumu                              | not "potlačenie hluku"                                                                                        |
| echo cancellation                     | potlačenie ozveny                            |                                                                                                               |
| push to mute                          | stlmenie stlačením                           | (unsure)                                                                                                      |
| hotkey                                | klávesová skratka                            |                                                                                                               |
| shortcut                              | skratka                                      |                                                                                                               |
| theme                                 | motív                                        | Windows SK                                                                                                    |
| dark                                  | tmavý                                        |                                                                                                               |
| light                                 | svetlý                                       | strength level "Slabé"                                                                                        |
| tip                                   | tip                                          |                                                                                                               |
| wizard                                | sprievodca                                   |                                                                                                               |
| import                                | importovať                                   |                                                                                                               |
| link                                  | odkaz                                        | channel link = "prepojenie"                                                                                   |
| favourite / favorite                  | obľúbené                                     |                                                                                                               |
| ping                                  | ping                                         |                                                                                                               |
| block                                 | zablokovať                                   | kept apart from ban                                                                                           |
| user                                  | používateľ                                   |                                                                                                               |
| relay                                 | relay                                        | stays English; masculine, declined like "server": relayu, relayom, relaye; "bez relayu", "s relayom" (unsure) |
| end-to-end encrypted                  | šifrované end-to-end / end-to-end šifrovanie | not "koncové/koncovo" (unsure)                                                                                |
| warning                               | varovanie                                    | "upozornenie" is reserved for notification                                                                    |
| online / offline                      | online / offline                             | indeclinable                                                                                                  |
| suppressed (server)                   | umlčaný / Zrušiť umlčanie                    | (unsure)                                                                                                      |
| extras (wizard)                       | funkcie navyše                               | avoids clash with doplnok (unsure)                                                                            |

### Slovenščina (sl)

Register: informal "ti" everywhere (tvoj, te, izberi, uporabi); never formal "Vi" (vaš, Uporabite, Izberite, ste). Buttons: 2nd person singular imperative, shortest form ("Shrani", "Pridruži se", "Namesti").

| English              | Slovenian                                 | Note                                                                                                                          |
| -------------------- | ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| channel              | kanal                                     |                                                                                                                               |
| sub-channel          | podkanal                                  |                                                                                                                               |
| server               | strežnik                                  |                                                                                                                               |
| server browser       | brskalnik strežnikov                      |                                                                                                                               |
| screen share         | deljenje zaslona                          |                                                                                                                               |
| share your screen    | deli zaslon                               |                                                                                                                               |
| camera               | kamera                                    |                                                                                                                               |
| watch party          | skupno gledanje                           |                                                                                                                               |
| player               | predvajalnik                              |                                                                                                                               |
| playlist             | seznam predvajanja                        | short tab may be "Seznam"                                                                                                     |
| plugin               | vtičnik                                   |                                                                                                                               |
| stage                | oder                                      |                                                                                                                               |
| whisper              | šepet (noun), šepetaj (verb)              | was mixed šepet/šepetanje/šepni                                                                                               |
| Push-to-talk         | Push-to-talk                              | glossary: keep English; capital P also mid-sentence; "tipka Push-to-talk"                                                     |
| voice activity       | glasovna dejavnost                        | not "aktivnost"                                                                                                               |
| mute                 | utišaj (mikrofon) / utišan                |                                                                                                                               |
| deafen               | izklopi zvok / zvok izklopljen            | not "oglušaj", "gluh"                                                                                                         |
| friend               | prijatelj                                 |                                                                                                                               |
| friend request       | prošnja za prijateljstvo                  |                                                                                                                               |
| direct message       | zasebno sporočilo                         | same as private message                                                                                                       |
| private message      | zasebno sporočilo                         |                                                                                                                               |
| message              | sporočilo                                 |                                                                                                                               |
| notification         | obvestilo                                 |                                                                                                                               |
| settings             | nastavitve                                |                                                                                                                               |
| profile              | profil                                    |                                                                                                                               |
| avatar               | avatar                                    |                                                                                                                               |
| badge                | značka                                    |                                                                                                                               |
| overlay              | prekrivni sloj                            | not "prekrivanje"                                                                                                             |
| join                 | pridruži se                               |                                                                                                                               |
| leave                | zapusti                                   |                                                                                                                               |
| connect              | poveži (se)                               |                                                                                                                               |
| disconnect           | prekini povezavo                          |                                                                                                                               |
| certificate          | potrdilo                                  | not "certifikat" (as Windows)                                                                                                 |
| ban                  | prepovej dostop / prepoved                | not "izobčenje", "izključiti" (unsure)                                                                                        |
| kick                 | izženi / izgon                            | "Izgni" is a wrong form; not "brcanje" (unsure)                                                                               |
| move                 | premakni                                  |                                                                                                                               |
| comment              | komentar                                  |                                                                                                                               |
| nickname             | vzdevek                                   |                                                                                                                               |
| username             | uporabniško ime                           |                                                                                                                               |
| register             | registriraj se / registriran              |                                                                                                                               |
| admin                | skrbnik                                   | currently left as "Admin" (change)                                                                                            |
| permission           | dovoljenje                                | permission names always infinitives ("Pridružiti se", "Izganjati", "Prepovedati dostop") in admin, servers, pluginPermissions |
| invite               | povabilo (noun), povabi (verb)            |                                                                                                                               |
| tray                 | sistemska vrstica                         |                                                                                                                               |
| pin                  | pripni / pripeto                          |                                                                                                                               |
| reaction             | odziv                                     |                                                                                                                               |
| emoji                | čustvenček                                | (unsure)                                                                                                                      |
| file                 | datoteka                                  |                                                                                                                               |
| upload               | naloži / nalaganje; bandwidth: pošiljanje | not "prenos navzgor"                                                                                                          |
| stream               | prenos                                    | was "tok" (change)                                                                                                            |
| viewer               | gledalec; image viewer: pregledovalnik    |                                                                                                                               |
| live                 | v živo                                    |                                                                                                                               |
| chat                 | klepet                                    |                                                                                                                               |
| mention              | omemba / omeni                            |                                                                                                                               |
| activity             | dejavnost                                 |                                                                                                                               |
| follow               | sledi                                     |                                                                                                                               |
| repository           | skladišče                                 | files mix with "repozitorij" (change)                                                                                         |
| install              | namesti                                   |                                                                                                                               |
| update               | posodobi / posodobitev                    |                                                                                                                               |
| volume               | glasnost                                  |                                                                                                                               |
| microphone           | mikrofon                                  |                                                                                                                               |
| speaker              | zvočnik (device), govorec (person)        |                                                                                                                               |
| headphones           | slušalke                                  |                                                                                                                               |
| noise suppression    | dušenje šuma                              | was mixed with zmanjševanje šuma / odstranjevanje hrupa                                                                       |
| echo cancellation    | odpravljanje odmeva                       |                                                                                                                               |
| push to mute         | pritisni za utišanje                      |                                                                                                                               |
| hotkey               | bližnjica                                 |                                                                                                                               |
| shortcut             | bližnjica                                 |                                                                                                                               |
| theme                | tema                                      |                                                                                                                               |
| dark                 | temna (tema)                              |                                                                                                                               |
| light                | svetla (tema)                             |                                                                                                                               |
| tip                  | namig                                     |                                                                                                                               |
| wizard               | čarovnik                                  |                                                                                                                               |
| import               | uvozi                                     |                                                                                                                               |
| link                 | povezava                                  |                                                                                                                               |
| favourite / favorite | priljubljeno / priljubljeni               |                                                                                                                               |
| controls             | kontrolniki                               | was "kontrole"                                                                                                                |
| relay                | relej (releja, releju)                    | SSKJ has "rele"; "relej" kept as used throughout (unsure)                                                                     |
| online / offline     | povezan / brez povezave                   | not "na spletu"                                                                                                               |
| voice activation     | glasovna aktivacija                       | distinct from voice activity = glasovna dejavnost (unsure)                                                                    |
| quotes               | »…«                                       | not „…“ or "…"                                                                                                                |
| menu path            | v razdelku Nastavitve → …                 | arrow, not ">"                                                                                                                |
| quit                 | zapri                                     |                                                                                                                               |

### Svenska (sv)

Register: informal "du/dig/din" everywhere (no "ni"), gender-neutral "personen"/"hen" instead of "han". Buttons: imperative verb ("Gå med", "Spara", "Bannlys"), never infinitive ("Bannlysa") or present tense ("Bannlyser").

| English                    | Swedish                                                  | Note                                                                          |
| -------------------------- | -------------------------------------------------------- | ----------------------------------------------------------------------------- |
| channel                    | kanal                                                    |                                                                               |
| sub-channel                | underkanal                                               |                                                                               |
| server                     | server (pl. servrar)                                     |                                                                               |
| server browser             | serverlista                                              | (unsure)                                                                      |
| screen share               | skärmdelning                                             |                                                                               |
| share your screen          | dela din skärm                                           |                                                                               |
| camera                     | kamera                                                   |                                                                               |
| watch party                | watch party (ett watch party, partyt)                    | loanword is normal; drop "tittarkväll" (unsure)                               |
| player                     | spelare                                                  | fix "Playerkontroller" → "Spelarkontroller"                                   |
| playlist                   | spellista                                                |                                                                               |
| plugin                     | tillägg                                                  | not leftover "plugin/pluginet/plugins"                                        |
| stage                      | scen                                                     |                                                                               |
| whisper                    | viska                                                    |                                                                               |
| Push-to-talk               | Push-to-talk                                             | stays English, always capitalised (also mid-sentence); "Push-to-talk-tangent" |
| voice activity             | röstaktivitet                                            |                                                                               |
| mute                       | stäng av mikrofonen (self); tysta (others/server)        | state: "mikrofon av"; drop "ljudlös", "Ljud av"                               |
| deafen                     | stäng av ljudet (self); döva / dövad (others/server)     | state: "ljud av"; avoid noun "dövning" (unsure)                               |
| friend                     | vän (pl. vänner)                                         |                                                                               |
| friend request             | vänförfrågan                                             |                                                                               |
| direct message             | direktmeddelande                                         |                                                                               |
| private message            | privat meddelande                                        |                                                                               |
| message                    | meddelande                                               |                                                                               |
| notification               | avisering                                                | as Android/Windows sv                                                         |
| settings                   | inställningar                                            |                                                                               |
| profile                    | profil                                                   |                                                                               |
| avatar                     | avatar                                                   |                                                                               |
| badge                      | märke                                                    | drop "bricka"                                                                 |
| overlay                    | överlägg                                                 | as Discord sv; not "overlay" (unsure)                                         |
| join                       | gå med                                                   | permission "Enter": "gå in" ok                                                |
| leave                      | lämna                                                    |                                                                               |
| connect                    | anslut / ansluten                                        |                                                                               |
| disconnect                 | koppla från / frånkopplad                                |                                                                               |
| certificate                | certifikat                                               |                                                                               |
| ban                        | bannlys / bannlysning                                    | "Blys/blyst/blysa" is not Swedish; "banna" means scold                        |
| kick                       | sparka ut                                                | drop "Kicka"                                                                  |
| move                       | flytta                                                   |                                                                               |
| comment                    | kommentar                                                |                                                                               |
| nickname                   | smeknamn                                                 |                                                                               |
| username                   | användarnamn                                             |                                                                               |
| register                   | registrera                                               |                                                                               |
| admin                      | admin / administratör                                    | "admin" in short labels                                                       |
| permission                 | behörighet                                               | drop "rättighet", "Ban-rätten"                                                |
| invite                     | inbjudan (pl. inbjudningar)                              |                                                                               |
| tray                       | systemfältet                                             |                                                                               |
| pin                        | fäst                                                     | pin a message/camera                                                          |
| pin version                | lås version / låst tillägg                               | plugin updates; not "fäst" (unsure)                                           |
| reaction                   | reaktion                                                 |                                                                               |
| emoji                      | emoji                                                    |                                                                               |
| file                       | fil                                                      |                                                                               |
| upload                     | uppladdning / ladda upp                                  |                                                                               |
| stream                     | sändning                                                 | Twitch string still English                                                   |
| viewer                     | tittare                                                  | image viewer: "bildvisare"                                                    |
| live                       | Live                                                     | Live rule: sv keeps "Live"                                                    |
| chat                       | chatt                                                    |                                                                               |
| mention                    | omnämnande / nämna                                       |                                                                               |
| activity                   | aktivitet                                                |                                                                               |
| follow                     | följ                                                     |                                                                               |
| repository                 | förråd                                                   |                                                                               |
| install                    | installera                                               |                                                                               |
| update                     | uppdatera / uppdatering                                  |                                                                               |
| volume                     | volym                                                    |                                                                               |
| microphone                 | mikrofon                                                 |                                                                               |
| speaker                    | högtalare (device); talare (person)                      |                                                                               |
| headphones                 | hörlurar                                                 |                                                                               |
| noise suppression          | brusreducering                                           |                                                                               |
| echo cancellation          | ekoreducering                                            |                                                                               |
| push to mute               | Push-to-mute                                             | like Push-to-talk (unsure)                                                    |
| hotkey                     | kortkommando                                             |                                                                               |
| shortcut                   | kortkommando                                             | drop "genväg"                                                                 |
| list (in compounds)        | lista / listan                                           | fix Norwegian "plugin-listen"                                                 |
| send                       | skicka                                                   |                                                                               |
| relay                      | relä (ett relä, reläet, pl. reläer)                      | "Mumpeer-relä"                                                                |
| setup (first-start wizard) | konfiguration / konfigurationsguiden                     | not "installation" (that is installing software) (unsure)                     |
| Auto (option)              | Automatisk; in hints "Automatiskt läge"                  | never bare "Auto"                                                             |
| permission names (lists)   | infinitive ("Bannlysa", "Återställa profiler", "Visa …") | not imperative                                                                |
| quotes                     | ”…”                                                      | never straight "…"                                                            |
