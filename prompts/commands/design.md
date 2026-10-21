---
description: Design a real website or app interface and show it in the preview pane
argument-hint: "<what to design>"
---
Design this: $@

Work like a product designer handing over a finished design, not a sketch.

1. **Decide what it is.** A website (landing page, marketing site, portfolio, docs, store) or a program's interface (app screen, dashboard, tool, settings, onboarding). If the request leaves it open, pick the one that fits and say so.

2. **Write the brief first**, in four short lines in your reply before any code:
   - **what**: the thing, and who it's for
   - **headline**: the one line the design is built around. For a site, the real hero headline it would ship with. For an app, the job the main screen does, said as a sentence. Everything on the page should serve this line
   - **direction**: a named visual direction in a few words (like "swiss editorial, ink on newsprint" or "dense terminal-grade dashboard, amber on graphite"), with the typefaces and 3 to 5 colours it uses
   - **the one detail**: the thing someone will remember about it

3. **Build it** as `index.html` in a new folder named after it, self-contained, then open it once with `preview`. It has to read as a real product:
   - **websites**: a full page. Nav, hero with the headline and a real subline, then the sections a site like this actually has (features or work, proof, pricing or details, a closing call to action), and a footer. Real copy throughout, written for this product
   - **programs**: the actual working screen. Its chrome (sidebar or toolbar, header), realistic data with believable names, numbers and dates, and the states that matter: selected, hover, empty or loading where they'd show. Make the main interaction work with a little JS if it's cheap to
   - type with a clear scale and pairing, chosen for this direction. Never default to Inter, Roboto, Arial or system-ui as the look
   - a palette built as CSS variables, one strong accent used sparingly
   - spacing on a rhythm, alignment that holds, density that suits the thing (marketing breathes, tools are tight)
   - responsive down to phone width
   - `generate_image` for any photography or illustration it needs, saved into the folder. No grey placeholder boxes

4. **Then look at it.** Open the file in the `browser` tool (a `file://` url) and take a screenshot, scroll and take more for a long page. Check it against the brief, fix what's off, and only then say you're done.

Avoid the stock AI look: purple-to-blue gradients, gradient blobs, glass cards on everything, emoji as icons, a centred hero with two buttons because that's the default, "Elevate your workflow" copy.

Finish with two lines: what you'd push further, and one alternative direction worth trying.
