# Changelog

All notable changes to Palette are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Installed apps self-update at launch, so most users are always on the latest release.

## [Unreleased]

### Changed

- English karaoke rises one syllable at a time, the way Korean and Japanese rise one character at a time. Letters used to take equal turns, so silent letters and commas rose on their own while the singer held a vowel, and the same word moved differently each time it repeated. Now "away," rises as "a" then "way," right when each is sung, every repeat looks the same, and each syllable snaps up quickly as it is sung. Korean and Japanese are unchanged.
- Held notes in word karaoke no longer sway with the singer's pitch. Letters just rise into place.

### Fixed

- The Lyric syncs panel's "words on time" no longer reads high on hard songs. On a song where the check doubts many lines, even the words it's sure of are right less often, and a line that starts well after its timestamp is off more often. The estimate now counts both. Against 79 hand-timed songs, the ones it scores under 93% used to average 88% estimated against 86% measured and now average 85%, the hardest song went from 81% to 72% against a measured 72%, and 70 of the 79 songs show the same percent as before or one point different. Scores update on their own; nothing needs relearning.

## [1.0.13] - 2026-11-09

### Changed

- Word karaoke shows the singer's voice. Each letter brightens and floats up into place as it's sung, the highlight flows smoothly through each line instead of jumping from syllable to syllable, and with the vocal separator installed, held notes lift and sway with the singer's pitch and loudness.
- The Lyric syncs panel shows how good each sync is: an estimated share of words on time for each song, a line map where the least sure lines stand tallest, what the second listen fixed, and a Least sure sort. Saved syncs relearn once on their next full listen to keep these details.
- Search is faster. "From your history" shows up within about a third of a second instead of waiting on album covers, and covers it found are remembered across restarts. History loads in 13 steps instead of 63 and filters several times faster, Library loads its lists side by side, and a search you ran in the last five minutes shows results at once.
- A window you can't see no longer receives the waveform. The pre-opened focus window and the widget behind focus mode used to get every update, about 30 times a second.
- Word karaoke uses about 1.3 GB less memory while it learns a song (a peak of about 2.3 GB instead of 3.5 GB) and gives most of it back as soon as the song is done instead of holding it for five minutes. Held notes reuse vocals the second listen already separated, so about half again as many get the singer's pitch and loudness at no extra cost.
- Lyrics take far less work to draw while karaoke plays: about 40% less main-thread time per second in the widget and in focus mode, with no change to how anything looks. Line changes no longer re-read every letter of the song, and a hidden, pre-opened focus window no longer animates karaoke nobody can see.
- Hidden windows stop animating. A Palette window you can't see used to keep drawing at full speed, because Windows never told its page it was hidden: the widget behind focus mode and the pre-opened focus window kept running karaoke, the progress bar and their status checks. Palette now tells each window when it's shown or hidden, and a hidden one does about half the work or less (the widget behind focus mode went from about 140–160 to about 66 ms of main-thread time per second). Everything catches up the instant the window shows.
- Palette uses less memory. Every window used to load the whole 5 MB Japanese and Korean font the moment it showed one character Geist lacks, even the ✓ in "Connected ✓". The font is now cut into Hangul, Japanese and symbol files, and a window loads only the ones it shows: about 18 MB less per window when nothing on screen is Korean or Japanese, about 10 MB less with Korean, about 8 MB less with Japanese. Text looks exactly the same.
- The widget's UI thread stays quiet while you're not touching it. The check that lets clicks pass through the widget's empty edges used to wake it about 85 times a second, and about 250 times a second with the pointer near the widget. It now reads the window's position straight from Windows.
- Word karaoke follows Japanese singing much more closely. It used to listen for every kanji in its Chinese reading (水面 as "shuimian", 一人 as "1 ren"), for っ as "tsu" and for きょ as "ki-yo". っ, small kana and ー are now heard as sung. With the optional Japanese dictionary installed beside the karaoke model, kanji and numbers get their Japanese readings and the particles は and へ are heard as "wa" and "e". On five Japanese songs, lines the aligner doubts went from 45% to 23%. Saved syncs relearn on each song's next full listen.

### Fixed

- Opening focus mode no longer flashes white for a few frames before the room fades in.
- Refreshing Search's "From your history" brings new songs instead of cycling the same few. It used to read only your newest 600 plays (about two days), so it had a couple dozen songs to choose from, and the last row of one refresh became the first row of the next. It now reads your whole history, remembers what it showed you in this part of the day, and won't show a song again until it runs out. Picks weigh how much you've played a song against how long it's been, so refreshes reach deeper favorites. "Forgotten favorite" and "On this day" now actually appear. Opening Search still shows the same set within a morning, afternoon or evening. "Something different" no longer suggests songs you played more than a couple of days ago.
- Word karaoke now learns most Japanese songs. Lyrics with a long-vowel bar (トートロジー), a repeat mark (悠々) or a kanji number (一人) used to make the whole song give up on word sync every time it played, so it stayed on line timing forever (Yorushika's あぶく, for one). Numbers written in digits (By 11, 7個目, 11년째) no longer stop a song either. If a line still has a word Palette can't sound out, only that line stays on line timing.

## [1.0.12] - 2026-11-08

1.0.11 was never published, so 1.0.12 also brings its changes: the last three items under Changed.

### Added

- Search has a section strip: Songs, Library and History. Tab moves between them, and `Ctrl+Alt+L` opens Search on Library.
- Library lists Liked Songs, then your playlists and the ones you follow, recently played first. The playlist that's playing says Playing, and inside it the current song says Now. Typing filters them instantly and finds public playlists on Spotify below. Enter opens a playlist's songs with Play in the header, Shift+Enter plays it, and Esc or the Library link above it goes back.
- History lists everything you've played, grouped by day with play times. The day's header stays pinned at the top as you scroll until the next day replaces it. Each day's header also shows how many songs you played that day and how long you listened. Enter plays a song and Shift+Enter queues it.
- Song credits after the lyrics: who wrote, produced, arranged, recorded, mixed and mastered the song, from Genius. They're grouped by kind (production, songwriting, performers, engineering), with people who share roles on one line, in one type size and two tones: roles in gray, names in white. Once the singing ends, the lyrics glide up to the credits and roll through them at a reading pace, timed to the outro; pausing stops the roll and scrolling takes over. Korean names show their romanization in a quieter tone, and "Credits from Genius" opens the song's Genius page. Paste a free Genius client access token in Preferences → Connectors to turn it on. Songs without a confident match show nothing.
- Each connector in Preferences now links to the page where you create its key (Last.fm, Genius) or manage the app (Spotify dashboard).

### Changed

- Palette now asks Spotify for permission to read your playlists, Liked Songs and recently played. Reconnect Spotify once (Search's Library shows the button) to use Library.
- While Search's "Something different" picks load, the section's title is a placeholder bar too, instead of a real title over empty rows.
- Search's selection, section tabs and Play button now move instantly with the arrow and Tab keys instead of fading behind them. Queueing a song still highlights its row, which now fades out instead of blinking off.
- Search's icons are redrawn at the exact size they're shown, so their lines sit on whole pixels and look sharp. Songs now has a music note instead of a second magnifier, and Library a stack of albums.
- Typing in History searches every song you've played, not just the ones already scrolled into view.
- Text in the widget and Search is sharper on Windows. It now uses ClearType instead of the softer grayscale smoothing that every line had before. The lyric fades at the top and bottom edges look the same but are drawn differently, and the lines around it keep ClearType too. Lines blurred by the depth-of-field effect, the line being sung word by word, and the current line during its brief light-up sweep still use grayscale.
- Holding an arrow key in Search's History now moves one row per key repeat, smoothly. It used to re-format every play time on each press and fall several rows behind.
- Album covers in the player, queue and Search are sharper. Palette now shrinks each cover to the exact pixels of its box with a high-quality filter instead of leaving it to the browser, and Spotify rows load the 300px cover instead of the 64px one, which was blurry at any Windows scaling above 100%.
- Skipping songs, seeking and switching sizes animate more smoothly. The song-change slide and the button presses now run on the graphics card, so they no longer stutter while the new song loads. Queue rows highlight under the cursor right away instead of fading in slowly. The queue panel, the right-click menu and the welcome bubble now grow slightly out of the widget, the cursor and the bubble's pointer as they fade in.
- Switching between lyrics and the queue (in the expanded player and focus mode) no longer shows queue rows printed over lyric lines. The old view fades and softens out quickly before the new one sharpens in, and in focus mode the lyrics no longer blink out when the queue opens or slide down from the top when it closes.
- Focus mode's big album cover is sharp now when Spotify is connected. It shows Spotify's 640px cover instead of the 300px one Windows hands Palette, which was blown up almost 2x.
- Lyrics are calmer: lines fade evenly the further they are from the current one, above and below.
- Focus mode: lyrics fill the right side of the screen.
- Lyric lines wrap more cleanly: short ad-libs like "(oh-oh)" stay on one row, and Korean words no longer split between syllables.
- The current lyric line is the only sharp, full-size line: the others are drawn a little smaller and go softly out of focus the further they are from it, and a new line lights up left to right as it starts. Hovering a line brings it into focus. The accent line beside it now moves with the lyrics as they scroll, so it stays put while each new line slides into it, and it stretches to cover a line that wraps onto two rows. It grows in when singing starts and shrinks away during instrumental breaks.
- Focus mode shows at most three lyric lines above the current one, and on tall screens the current line sits higher to match.
- Karaoke word sync now double-checks itself. Palette measures how sure it is of each word, gives the lines it doubts a second listen on just the vocals, separated from the music, and keeps whichever version it's surest of. On a 79-song test set, words more than 0.3 s off dropped from 5.1% to 3.6%. The second listen needs the Kim Vocal 2 model beside the karaoke model and adds up to about a minute and a half of background work per song. Saved syncs keep showing and relearn the next time each song is played in full.
- Lyrics outside focus mode are a size up, 17px instead of 16px, so most lines fill more of the row. Rows keep the same height, so the panel shows as many lines as before.
- Album covers have a faint light edge, so near-black covers no longer melt into the dark widget.
- In the expanded player the progress bar sits right on top of the play controls instead of floating closer to the lyrics.
- Karaoke word sync is more accurate at the end of songs, before instrumental breaks, on lines that repeat a phrase, and on "ooh"/"ah" adlib lines. Saved syncs keep showing and quietly relearn the next time each song is played in full.
- "Something different" suggestions load much faster. Lookups run side by side, connections are reused, and results are remembered on disk, so a song you play often costs no network at all the next time. When suggestions can't load, the search window now says so instead of quietly leaving the section out.
- Text uses Geist, with Pretendard for Korean and Japanese (so Japanese lyrics no longer mix two fonts), and reads a little firmer on the see-through window.

### Fixed

- Palette's hotkeys now work inside League of Legends and other games that turn off Windows hotkeys while they're focused. Palette now also listens for its chords the way Discord's keybinds do, so they keep working in those games.
- A lyric line no longer jumps from one row to two when it becomes the current line.
- With karaoke words on, the lyrics move to the next line as soon as its first word starts lighting up. Long lines used to stay highlighted after they finished while the next line was already filling.
- Karaoke words on Spotify no longer start up to 0.4 s early on some plays and on time on others. Spotify reports each new song as starting a little before its audio really does, and Palette used to keep that head start for the whole song, by a different amount every play. Now it eases back onto Spotify's timeline in the first couple of seconds, so one word-timing setting fits every song.
- Karaoke words no longer race through each line with no rhythm on songs Palette learned while it couldn't hear the music (for example when Windows wouldn't let it capture Spotify's audio). Those songs now show line karaoke and relearn word timing the next time they play in full, and a listen that doesn't pick up the vocals is no longer saved.
- Skipping a song no longer plays its entrance slide twice. The song's length arrives about half a second after its title, and Palette used to treat that as a second song change.

## [1.0.10] - 2026-10-28

### Fixed

- Album art that fails to load when a song starts now keeps retrying with backoff for about two minutes, and again whenever the player updates the song's details, instead of staying a blank note for the whole track.
- Long track titles in the pill now end in an ellipsis before the waveform instead of running into it.

## [1.0.9] - 2026-10-24

### Fixed

- Search's "Something different" and the queue's more-like-this work again. Last.fm stopped returning similar tracks, so Palette now falls back to similar artists and suggests their top tracks.

## [1.0.8] - 2026-10-20

### Fixed

- Search's "Something different" section no longer goes empty for long listening histories. It now looks further down Last.fm's similar-tracks list and tries more of your songs before giving up, and Palette's log records why the section came up empty.

## [1.0.7] - 2026-10-08

### Added

- "Restart Palette" in the tray menu, for when the window gets stuck (for example, painting black) and needs a fresh start.
- With nothing playing, you can still change the widget's size and open your queue. The expanded view shows the queue under "Nothing playing" instead of an empty box.

### Fixed

- Removing one copy of a song queued twice removes that row. It used to take out the first copy, and could drop the song Spotify was about to play.
- Removing a song Spotify has already been handed says it will still play, instead of looking removed.
- Next no longer does nothing when the next queued song is the one already playing. It plays that song again from the start.

## [1.0.6] - 2026-10-03

### Fixed

- Spotify always takes priority over Apple Music and browser/video sessions, even when paused. Watching or clicking YouTube no longer switches Palette away from an available music app; its display and playback controls use the same priority.

## [1.0.5] - 2026-09-10

Korean karaoke fills for as long as the syllable is sung.

### Changed

- Hangul, kana, and CJK lyric tokens crawl their measured span instead of flashing full in 90 ms. Short English words (`one`, `I`, `oh`) still punch in.

## [1.0.1] - 2026-09-03

A patch for installed 1.0 copies: the focus queue is a setlist again, and launch/playback got lighter.

### Changed

- Focus mode's queue is a reading-width setlist next to the art (room type, duration on the artist line) instead of the widescreen pane that stretched rows to the screen edge.
- Launch and playback are cheaper. Search and Preferences warm after first paint. Lyrics fetches are capped and joined. Position-only media ticks skip the art marshal. Queue reorder no longer re-renders on every pointer move. Expanding toward focus starts warming the takeover window.

### Fixed

- The living-separator preference is applied before any window votes, so a realm no longer flashes the default before the saved setting lands.

## [1.0.0] - 2026-08-31

Palette 1.0. The widget you have been running since 0.7 is the product: free placement, synced lyrics, Search, queue, focus mode, and Spotify Connect. This cut lands the last reliability and polish fixes from the soak, and stamps the version that matches what it already is.

### Added

- Focus mode's open queue becomes a wide right pane (~62%) at ≥1100px, with full-width rows and trailing duration. Narrower takeovers keep the compact art-capped column.

### Fixed

- Search play no longer dismisses and goes silent on failure. `no_device`, `busy`, and `diverged` stay on the Search footer so a dead click is honest. Queue play-now uses the same status copy and no longer caches a failed URI forever.
- Search's empty state seats all seven picks again (4 history + 3 discovery) at the born-at-size 52px row height. History picks without a disk thumb fall back to a Spotify cover instead of the note glyph.
- Queue and Search feedback lines wrap on their own row instead of fighting chrome and clipping mid-word.
- The taskbar no longer covers the widget's bottom edge after switching apps. Palette re-asserts topmost while visible so the shell cannot permanently raise the taskbar over a flush-bottom park.
- Search's "From your history" picks and the queue's Earlier feed no longer surface YouTube videos, anime, or other browser sessions. GSMTC often labels those as Music, so the previous video/image filter let them through; history now keeps Apple Music and Spotify listens only. Already-logged video rows disappear from those surfaces without clearing history.
- A long sung hold no longer jumps the lyric highlight onto the five-dot rest row. LRC only has line starts, and the guessed ≤5s hold was inserting an instrumental break mid-phrase whenever the next stamp was far enough away. Rest dots now appear only for an uploader empty-timestamp marker (and the intro before the first line).

## [0.7.5] - 2026-07-28

A small follow-up to 0.7.4's track-change work: the slide is quicker, and the expanded album view's status line stops moving the waveform.

### Changed

- The expanded album view now seats "No synced lyrics" directly under the artist line — the same place focus mode puts it — and centres the waveform between that line and the progress bar. The caption no longer pushes the bars down, and neither does the "Playing on <device>" tag: the two share one row, the caption answering first and the tag settling into the same seat as it fades.
- Track-change slides are quicker. A skip now completes in one short beat rather than an exit trailing underneath a longer entrance, so skipping repeatedly no longer feels weighed down — a single skip still reads directionally. The album cover's crossfade was brought to the same speed so nothing outlives the swap it belongs to.

## [0.7.4] - 2026-07-23

The placement + track-change release: the widget goes anywhere and stays put, and track changes got one calm, directional grammar.

### Changed

- Placement is free: drag the widget anywhere on screen and it stays exactly where you dropped it. Drop it near an edge and only that axis tidies to a 12px margin — the other keeps its position. The corner magnet is gone.
- The bottom edge offers two snap lines — above the taskbar, or flush with the screen — whichever you're nearer.
- Palette no longer moves itself when another app goes fullscreen. The widget stays where you put it, and the reachable area is the same everywhere. "Hide on fullscreen" is unaffected.
- Track changes slide directionally: skipping forward slides the title (and the big album art, where it's the hero) out left and in right; previous mirrors it. A change Palette didn't initiate drifts forward.
- The waveform no longer runs its track-change announcement — the collapse-to-a-single-dot ladder read as the widget resetting at focus mode's scale. The bars now ride straight through every track change; the sliding title is the track-change beat. (Play/pause settle and bloom are unchanged.)
- Switching tracks no longer flashes the expanded/focus layout through the album or fallback view while lyrics load — the seated view holds until the lyrics verdict lands (or a short grace expires).

### Fixed

- Pill and card modes could throw the widget up to 392px away from where it was released when a drag crossed the middle of the screen.
- A skip's piecemeal metadata delivery could rubber-band the outgoing album cover during the track-change slide.

## [0.7.3] - 2026-07-19

The 1.0-readiness release: the feature arc since 0.7.2 plus a full audit pass — robustness, security, and accessibility hardening across the backend and every window.

### Added

- `Playing on <device>` tag: when Spotify plays on a phone or speaker while this PC is only the controller, the card, expanded, and focus views say where the audio is — explaining a quiet waveform.
- Search's empty state now recommends: "From your history" resurfacing picks plus "Something different" Last.fm-powered discoveries, rotating through the day, with pull-to-refresh (`↑` at the top slot).
- Queue rows added from history carry the album cover, with a glyph fallback for dead art URLs.
- Uninstalling always removes the Spotify tokens from disk; play history, preferences, and caches survive unless the uninstaller's "Delete the application data" checkbox is ticked.
- A Content-Security-Policy on every window.
- Accessibility: track changes are announced to screen readers; history rows can be played now from the keyboard (`Shift`+`Enter`); the hotkey-rebind capture, the right-click menu, and the first-run bubble are fully keyboard-operable.

### Changed

- Focus mode's waveform returned to the horizon — 19 audio-reactive capsules on their own band between the identity stack and the console — and the focus queue became a room-scale content column seated in the lyric column.
- The "Launch mode" preference now actually governs which mode the widget wakes in; the "Seek amount" preference was removed (the UI buttons always hardcoded ±10s — only the hotkeys ever read it).
- Offline is now told apart from missing: lyric captions read "Lyrics unavailable — offline" vs "No synced lyrics" (offline is never cached as a miss), and instructional hints got an AA contrast lift.
- The expanded album view floats its "No synced lyrics" caption between the artist line and the wave instead of leaning the bars into it.
- A design polish pass: one-line alignment across preferences, search, and the queue; bright keycap hint glyphs in Search; and a 16-fix motion sweep.
- Hotkey seeks now spin the transport glyph, exactly like clicking the button.
- Launch got lighter: the play-history index builds in the background instead of on the launch path, and the hidden widget's click-through watcher wakes far less often.
- Rebinding a hotkey now suspends the live shortcuts while capturing, so a currently-bound combo is captured instead of firing its action; modifier-less and shift-only chords are refused (they would swallow ordinary typing).
- Card mode is 6px taller so the album art no longer overflows its row.
- Release logs no longer record the name of the app that was fullscreen (kept to debug builds only).

### Fixed

- In-song silence no longer collapses and re-blooms the separator bars — a settle grace holds them while the track is still playing.
- The separator demotes a silent process capture to the device fallback instead of stranding the bars at zero.
- A rare freeze where a Windows media read on the wrong thread could wedge every in-app control while hotkeys kept working.
- Seeking a short distance — such as clicking a nearby lyric line — no longer flashes the position and lyric highlight backward before catching up.
- Clearing play history now empties the open queue and history feed immediately, not just on the next launch.
- A render error in any window now shows a working "Reload" button instead of an overlay whose clicks fell through to the desktop.
- The queue popover no longer let clicks land on the desktop (or start a window drag) in the gap above a short list.
- Spotify no longer forces a full re-login on a transient rate-limit or an ambiguous error — only genuine dead-session proof clears the tokens.
- Broad robustness hardening: config writes flush to disk before the atomic rename (crash-safe), network responses are size-bounded, internal in-flight guards survive a panic, and Spotify token-file writes are serialized.

### Security

- Trimmed window capabilities to what the app actually uses (dropped the unused open-URL / reveal-path and window-geometry grants), and the generic settings writer now allow-lists keys and caps value size.
- CI and release workflows pin their actions by commit SHA and run with a least-privilege token.

## [0.7.2] - 2026-07-14

The Palette release — the app is now **Palette**.

### Changed

- Renamed the app to **Palette** (installer, tray, windows); the `Ctrl+Alt+S` summon-search is now **Search**. The installer is keyed by product name, so self-updating from a "Pulse" install leaves the old entry side-by-side — uninstall "Pulse" once and re-toggle Start at login.
- Dropped "Segoe UI Variable Display" from the font stack — it rendered thin at UI sizes.
- Recomposed the focus room around the song block.
- The living separator's settle now exits outside-in and its bloom enters inside-out at every size; Search rows swap their "because you played" reason for controls on hover.
- Instrumental-break dots show only for the active break, the row collapses when it ends, and fills fade right-to-left.

### Added

- Preferences window (tray "Preferences…", with "Shortcuts / Help" jumping to hotkeys): Spotify and Last.fm connectors, rebindable global hotkeys (overrides persist; a system-reserved combo reports its failure), playback and general settings, and clearing play history.
- Onboarding: an empty-state nudge, a first-run bubble, and a right-click menu — plus honest gating: Spotify-powered features show an in-app Connect button, and "more like this" hides without a Last.fm key.
- Release logging: a rotating log file with panic capture, plus tray "Open logs"; config writes are now atomic, so a crash mid-write can't truncate settings.
- Housekeeping for the 1.0 push: MIT license, a real README, this changelog, and a CI build+test workflow.

### Fixed

- The media loop could wedge inside an unanswered Windows media read, freezing now-playing while the controls stayed alive — narrowed locking, per-call timeouts, and a watchdog.
- The recurring UI-thread "Application Hang": blocking media reads moved off the main thread at the hotkey and tray entry points, with a liveness watchdog.
- A React render throw can no longer strand the widget — an error boundary and global handlers catch it.
- The history feed could latch stuck after a failed fetch.

## [0.7.1] - 2026-07-13

### Changed

- Play history and up-next surfaces now show music only — podcasts, videos, and other non-music sessions are filtered out.
- The audio-reactive waveform is scoped to the app that is actually playing, so it no longer reacts to Discord voice or game sound effects.

### Added

- Flexible docking: place the widget freely anywhere on screen, with a corner magnet, edge rails, and a dedicated fullscreen seat that clears game HUDs and the taskbar.
- Corner-change glide: moving the widget to a different corner animates its layout to the new seat.

## [0.7.0] - 2026-07-12

The "hands + room" arc.

### Added

- Summon palette (`Ctrl+Alt+S`): a search-and-play command palette — Enter plays now, Shift+Enter queues to up-next, and from silence it starts playback outright.
- Focus mode: a fullscreen now-playing takeover ("Expand to focus") with room-scale lyrics and visualizer views.
- More-like-this: Last.fm similar tracks feed the up-next list.
- Instrumental-break countdown: five accent dots mark instrumental gaps in the lyrics view.

### Changed

- The expanded view now crossfades three peer layers — lyrics, album, and queue — under a fixed now-playing header.

## [0.6.4] - 2026-07-11

### Changed

- Queue-aware skip: pressing next (transport or `Ctrl+Alt+N`) lands on the up-next front when one exists, instead of the player's own next track.

## [0.6.3] - 2026-07-11

### Changed

- Re-seated the queue toggle out of the mode-bracket cluster — bottom-left in card/expanded, and in the pill's hover scrim beside play/pause.

## [0.6.2] - 2026-07-10

### Fixed

- Spotify writes failed with HTTP 411 — POST requests now send a `Content-Length` header, which Spotify's edge requires.

## [0.6.1] - 2026-07-10

### Fixed

- History rows are now actionable — track enrichment and on-demand search resolution wire up play-now and add-to-queue.

## [0.6.0] - 2026-07-10

Queue, history, Spotify, and the presence layer.

### Added

- Up-next queue and play history, surfaced through the "11a" queue & history UI (a corner-aware popover and an in-expanded peer layer).
- Spotify connection via OAuth (PKCE + loopback redirect) from the tray — queue read and play-history enrichment.
- Courtesy conceal: the widget auto-hides while fullscreen content owns its monitor and restores exactly when the episode ends (tray "Hide on fullscreen"); manual intent always wins.
- Resting pulse: a breathing dot for the no-session state, plus a track-change announcement in the living separator.

### Changed

- Lyrics picker prefers original-script (hangul/CJK/kana) synced entries over romanized uploads.
- The shell is now opaque — translucency without blur read as a hole.
- Icon press feedback: a skip pass-through flick and a mode-bracket verb pulse.

## [0.5.1] - 2026-07-09

### Changed

- The window never resizes: it is born at maximum size and every mode change is a CSS glide inside it, with cursor-polled click-through keeping the oversized gutter from eating desktop clicks. Removes per-frame animation shake from resizing the native window.

## [0.5.0] - 2026-07-09

### Changed

- Mode resizing finally feels native — a fixed content plane, frame pacing, and opacity-only enters (also folds in the earlier mock window frame and the animation-spec gap pass).

## [0.4.1] - 2026-07-09

### Changed

- Aligned the anchored mode-control cluster with every mode's transport centerline.

## [0.4.0] - 2026-07-08

### Added

- Expanded view toggle: show album art over lyrics on demand, with a nine-bar living-separator hero.

### Changed

- Dropped the "Now" label from the lyrics return-to-now chip.

## [0.3.0] - 2026-07-08

### Added

- Anchored mode controls: a fixed bottom-right collapse/expand bracket cluster stepping pill ↔ card ↔ expanded.

### Changed

- Tuned the living separator tighter to the music.

## [0.2.2] - 2026-07-08

### Added

- The expanded art view promotes the living separator to a seven-bar hero.

### Fixed

- Cut lyric first-appearance latency with a parallel fetch ladder and latest-wins resolution.

## [0.2.1] - 2026-07-08

### Added

- Tray "Check for updates" with in-label progress feedback.
- Lyrics browsing: a "Now" chip, magnetic re-latch, and a 2s idle resume.

## [0.2.0] - 2026-07-08

First public release.

### Added

- The always-on-top mini player (pill ↔ card ↔ expanded) controlling whatever is playing on Windows through the system media API (GSMTC): Apple Music, Spotify, and browsers.
- Synced lyrics (LRCLIB with disk cache), album-art adaptive accents, an audio-reactive "living separator", corner docking, morphing icons, and a monotonic position clock owning playback time.
- Ships as a per-user NSIS installer — single-instance, opt-in start-at-login — with a GitHub Releases auto-update pipeline and an in-app updater.

[Unreleased]: https://github.com/thientran01/palette/compare/v1.0.5...HEAD
[1.0.5]: https://github.com/thientran01/palette/compare/v1.0.4...v1.0.5
[1.0.1]: https://github.com/thientran01/palette/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/thientran01/palette/compare/v0.7.5...v1.0.0
[0.7.5]: https://github.com/thientran01/palette/compare/v0.7.4...v0.7.5
[0.7.4]: https://github.com/thientran01/palette/compare/v0.7.3...v0.7.4
[0.7.3]: https://github.com/thientran01/palette/compare/v0.7.2...v0.7.3
[0.7.2]: https://github.com/thientran01/palette/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/thientran01/palette/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/thientran01/palette/compare/v0.6.4...v0.7.0
[0.6.4]: https://github.com/thientran01/palette/compare/v0.6.3...v0.6.4
[0.6.3]: https://github.com/thientran01/palette/compare/v0.6.2...v0.6.3
[0.6.2]: https://github.com/thientran01/palette/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/thientran01/palette/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/thientran01/palette/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/thientran01/palette/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/thientran01/palette/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/thientran01/palette/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/thientran01/palette/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/thientran01/palette/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/thientran01/palette/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/thientran01/palette/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/thientran01/palette/releases/tag/v0.2.0
