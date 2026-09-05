# Lyric spotlight visual pass

User requested a minimal, neutral take on Apple Music lyric emphasis: slightly larger active type, gradient/blur and a faint shimmer; lyric text stays white rather than accent-colored.

Base lyrics now use stable 18px semibold type and more breathing room. The inner ink scales from .97 to1 with the existing 220ms in-out token, while row layout remains at its full size. Far context uses .45px static blur; active, hovered and manually browsed rows clear it. Both base and focus use distance-based tone. The current marker is a neutral hairline. A brief white gradient peak follows the existing word wipe, with no new timer or ambient loop. On light theme the peak stays foreground ink. Reduced-motion CSS disables scale, blur and glint while retaining essential timing highlights.

Timing calculations, caches, overlapping backing-vocal behavior, source text and scrolling stamps are unchanged. Font weight stays constant across line activation to avoid rewrap. The new driver style properties are cleared on deactivation/disposal; the glint inherits an active-row peak so seeking mid-word cannot leave a white stripe on old lyrics.

Validation: 46 frontend tests and production build pass. Browser screenshot/DOM preview verifies active18px/600, scale1, distant scale.97/blur.45px, and zero overlaps across27 mock rows. Reduced-motion guard is implemented in CSS; OS preference switching was not emulated. Four review perspectives identified two confirmed issues (stale sheen, hover specificity); both fixed. Native visual feedback remains pending.

Native visual feedback: 600 weight felt too thick and less modern. Revised row weight to500 (medium), retaining18px size, spacing and paint-only emphasis. Weight remains constant across activation.


## Silver-light revision

User asked to push the direction further while keeping it light. Typography is now450 weight. The active row has a faint neutral radial light field; sung words have a260ms onset bloom that releases using the existing playback clock/RAF. It freezes on pause, resets on seek and clears through active-row color gating. No extra animation loop or timing changes. Reduced motion disables both additions.

47 frontend tests pass, including bloom release, paused state, seek and disposal; production build passes. Browser preview confirms450 weight and the expected active-row radial field. Native appearance feedback remains pending.
