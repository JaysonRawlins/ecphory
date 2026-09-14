#!/usr/bin/env python3
"""Generator for docs/assets/heal-cycle.gif (the README self-correction figure).

Three acts:
  1. a query misses    - target episode is not in the top 5
  2. rate_search(miss) - the failed query is appended VERBATIM to the
                         episode's search_phrases (no LLM, exactly one phrase)
  3. the same query    - target now ranks; a resolution_log row is written

Regenerate when the constants below drift from the source:
    python3 docs/assets/heal_cycle_gif.py docs/assets/heal-cycle.gif

Constants shown in the figure, and where they live:
  CORRECTION_K = 5          src/service.rs   (top-k the heal validates against)
  MAX_SEARCH_PHRASES = 8    src/service.rs   (per-episode phrase cap)
  phrases boost 2.0x        src/index.rs     (vs name 1.5x, content 1.0x)
  origin: organic           src/recorder.rs  (SearchOrigin, default variant)
  resolution_log            src/store.rs     (absent from prune_logs by design)

The depicted query and episode are real strings from the author's store, but
the figure illustrates the MECHANISM - it is not a replay of a logged heal.
Result rows 2-5 are deliberately anonymous bars rather than invented episode
names, so no ranking is claimed that was not measured.

Requires Pillow. Fonts fall back gracefully off macOS, but the figure was
laid out against Menlo and may need width tuning elsewhere.
"""
import sys
from PIL import Image, ImageDraw, ImageFont

OUT = sys.argv[1] if len(sys.argv) > 1 else "heal-cycle.gif"

W, H = 720, 470
ACT = 20                      # frames per act
FRAMES = ACT * 3
DURATION_MS = 85

BG = (13, 17, 23)
PANEL = (22, 27, 34)
HAIR = (33, 38, 45)
CYAN = (56, 189, 248)
GREEN = (63, 185, 80)
RED = (248, 81, 73)
BLUE = (88, 166, 255)
AMBER = (210, 153, 34)
DIM = (110, 118, 129)
DIMMER = (72, 79, 88)
WHITE = (230, 237, 243)


def font(sz, bold=False):
    faces = ["/System/Library/Fonts/Menlo.ttc",
             "/System/Library/Fonts/SFNSMono.ttf",
             "/System/Library/Fonts/Supplemental/Courier New Bold.ttf",
             "/System/Library/Fonts/Helvetica.ttc"]
    for p in faces:
        try:
            return ImageFont.truetype(p, sz, index=1 if (bold and p.endswith(".ttc")) else 0)
        except Exception:
            continue
    return ImageFont.load_default()


F_TITLE = font(21, bold=True)
F_STEP = font(12)
F_LBL = font(13)
F_SM = font(11)
F_TINY = font(10)
F_CAP = font(15)
F_BADGE = font(15, bold=True)


def center(d, xy, text, fnt, fill):
    b = d.textbbox((0, 0), text, font=fnt)
    d.text((xy[0] - (b[2] - b[0]) / 2, xy[1] - (b[3] - b[1]) / 2 - b[1]), text, font=fnt, fill=fill)


def wide(d, text, fnt):
    b = d.textbbox((0, 0), text, font=fnt)
    return b[2] - b[0]


def ease(x):
    """smoothstep 0..1"""
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


# ---- content (real strings from the store; no fabricated rankings) ----
QUERY = "github release page has nothing to download"
EP_NAME = "ecphory v0.3 release engineering"
EP_ID = "019f5747"
PHRASES = [
    "how to release ecphory new version push button",
    "brew install ecphory homebrew tap setup",
    "cargo dist maintained abandoned status",
]

# geometry
QX0, QX1, QY = 28, 352, 96
RES_X0, RES_X1, RES_Y0 = 28, 352, 168
ROW_H = 30
CARD_X0, CARD_X1, CARD_Y0, CARD_Y1 = 372, 692, 96, 330
PHRASE_W = 288


def steps(d, act, t):
    """three-stage progress pills across the top"""
    names = ["1  search", "2  rate: miss", "3  same query"]
    cols = [CYAN, AMBER, GREEN]
    x = 28
    for k, (nm, c) in enumerate(zip(names, cols)):
        w = wide(d, nm, F_STEP) + 22
        on = (k == act)
        d.rounded_rectangle([x, 52, x + w, 74], radius=11,
                            fill=PANEL if on else BG,
                            outline=c if on else HAIR, width=2 if on else 1)
        center(d, (x + w / 2, 63), nm, F_STEP, WHITE if on else DIMMER)
        if k < 2:
            d.line([(x + w + 8, 63), (x + w + 20, 63)], fill=HAIR, width=1)
            d.polygon([(x + w + 20, 63), (x + w + 15, 60), (x + w + 15, 66)], fill=HAIR)
        x += w + 28
    return


def query_box(d, act, t):
    accent = CYAN if act != 1 else DIMMER
    d.rounded_rectangle([QX0, QY, QX1, QY + 46], radius=8, fill=PANEL, outline=accent, width=2)
    d.text((QX0 + 12, QY + 7), "query", font=F_TINY, fill=DIM)
    d.text((QX0 + 12, QY + 23), QUERY, font=F_SM, fill=WHITE if act != 1 else DIM)
    # a pulse travelling the box edge on act 0 and 2 to imply "running"
    if act in (0, 2):
        px = QX0 + 2 + ((t * 1.0) % 1.0) * (QX1 - QX0 - 4)
        d.line([(px, QY + 46), (min(px + 28, QX1 - 2), QY + 46)], fill=accent, width=2)


def results(d, act, t):
    """5 result rows. Target episode is absent in act 0/1, rank 1 in act 2."""
    d.text((RES_X0, RES_Y0 - 20), "top 5   (CORRECTION_K = 5)", font=F_TINY, fill=DIM)
    appear = ease((t - 0.15) / 0.5) if act == 2 else 0.0

    for r in range(5):
        y = RES_Y0 + r * ROW_H
        is_target = (act == 2 and r == 0)
        box_out = GREEN if is_target else HAIR
        d.rounded_rectangle([RES_X0, y, RES_X1, y + ROW_H - 6], radius=5,
                            fill=PANEL, outline=box_out, width=2 if is_target else 1)
        d.text((RES_X0 + 9, y + 6), str(r + 1), font=F_SM, fill=DIM)

        if is_target:
            d.text((RES_X0 + 28, y + 6), f"{EP_ID}  release engineering", font=F_SM, fill=GREEN)
        else:
            # anonymous placeholder bars: we are not claiming which episodes ranked
            bw = [150, 186, 132, 168, 144][r]
            fade = int(58 + 10 * ((r + int(t * 3)) % 2))
            d.rounded_rectangle([RES_X0 + 28, y + 9, RES_X0 + 28 + bw, y + 15],
                                radius=3, fill=(fade, fade + 6, fade + 12))

    # verdict badge under the list
    by = RES_Y0 + 5 * ROW_H + 6
    if act in (0, 1):
        blink = (t * 3) % 1.0 < 0.62 if act == 0 else True
        d.text((RES_X0, by), "✗  MISS", font=F_BADGE, fill=RED if blink else (96, 40, 38))
        d.text((RES_X0 + 84, by + 3), "target not in top 5", font=F_SM, fill=DIM)
        if act == 0:
            d.text((RES_X0, by + 34), "search_log  <-  taped   (origin: organic)",
                   font=F_SM, fill=DIMMER)
    else:
        a = ease((t - 0.3) / 0.4)
        if a > 0:
            col = tuple(int(BG[i] + (GREEN[i] - BG[i]) * a) for i in range(3))
            d.text((RES_X0, by), "✓  rank 1", font=F_BADGE, fill=col)
            if a > 0.7:
                d.text((RES_X0 + 100, by + 3), "resolution_log <- held", font=F_SM, fill=DIM)
            if a > 0.9:
                d.text((RES_X0, by + 34), "ecphory eval --heals   (prune-exempt, replayed forever)",
                       font=F_SM, fill=DIMMER)


def card(d, act, t):
    """the target episode + its search_phrases; act 1 appends one verbatim."""
    hot = act == 1
    d.rounded_rectangle([CARD_X0, CARD_Y0, CARD_X1, CARD_Y1], radius=8,
                        fill=PANEL, outline=AMBER if hot else HAIR, width=2 if hot else 1)
    d.text((CARD_X0 + 14, CARD_Y0 + 11), "episode " + EP_ID, font=F_TINY, fill=DIM)
    d.text((CARD_X0 + 14, CARD_Y0 + 27), EP_NAME, font=F_LBL, fill=WHITE)
    d.line([(CARD_X0 + 14, CARD_Y0 + 50), (CARD_X1 - 14, CARD_Y0 + 50)], fill=HAIR, width=1)

    n_now = len(PHRASES)
    d.text((CARD_X0 + 14, CARD_Y0 + 60), "search_phrases", font=F_TINY, fill=DIM)
    d.text((CARD_X1 - 92, CARD_Y0 + 60), "boost 2.0x", font=F_TINY, fill=CYAN)

    py = CARD_Y0 + 80
    for i, p in enumerate(PHRASES):
        txt = p if wide(d, p, F_TINY) < PHRASE_W else p[:44] + "…"
        d.text((CARD_X0 + 20, py + i * 22), "- " + txt, font=F_TINY, fill=DIM)

    # the appended phrase
    ny = py + n_now * 22
    if act == 1:
        a = ease((t - 0.42) / 0.34)
        if a > 0:
            col = tuple(int(PANEL[i] + (GREEN[i] - PANEL[i]) * a) for i in range(3))
            off = int((1 - a) * 14)
            txt = QUERY if wide(d, QUERY, F_TINY) < PHRASE_W else QUERY[:44] + "…"
            d.text((CARD_X0 + 20, ny + off), "+ " + txt, font=F_TINY, fill=col)
    elif act == 2:
        txt = QUERY if wide(d, QUERY, F_TINY) < PHRASE_W else QUERY[:44] + "…"
        d.text((CARD_X0 + 20, ny), "+ " + txt, font=F_TINY, fill=GREEN)

    used = n_now + (1 if act == 2 or (act == 1 and (t - 0.42) > 0) else 0)
    d.text((CARD_X0 + 14, CARD_Y1 - 26),
           f"{used} / 8 phrases   (MAX_SEARCH_PHRASES)", font=F_TINY,
           fill=AMBER if hot else DIMMER)


def link(d, act, t):
    """act 1: the rate call carrying intended_episode_ids to the episode."""
    y = 300
    if act != 1:
        return
    a = ease(t / 0.44)
    d.text((RES_X0, y + 44), "rate_search(miss,", font=F_SM, fill=AMBER)
    d.text((RES_X0 + 14, y + 60), f"intended_episode_ids=[{EP_ID}])", font=F_SM, fill=AMBER)
    x0, x1 = 250, CARD_X0 - 6
    xn = x0 + (x1 - x0) * a
    d.line([(x0, y + 52), (xn, y + 52)], fill=AMBER, width=2)
    if a > 0.02:
        d.polygon([(xn, y + 52), (xn - 7, y + 48), (xn - 7, y + 56)], fill=AMBER)
    if a >= 1.0:
        d.text((x0 + 6, y + 30), "append verbatim - no model call", font=F_TINY, fill=DIM)


CAPTIONS = [
    ("a query comes back empty-handed - the right episode never made the top 5", DIM),
    ("the failed query itself becomes the episode's next search phrase", AMBER),
    ("same words, now rank 1 - and kept as a permanent regression test", GREEN),
]


def frame(i):
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    act, t = i // ACT, (i % ACT) / ACT

    d.text((28, 20), "ecphory", font=F_TITLE, fill=WHITE)
    d.text((124, 25), "search  ->  rate  ->  heal", font=F_LBL, fill=CYAN)
    d.text((W - 250, 25), "the store learns the words you used", font=F_TINY, fill=DIMMER)

    steps(d, act, t)
    query_box(d, act, t)
    results(d, act, t)
    card(d, act, t)
    link(d, act, t)

    cap, col = CAPTIONS[act]
    d.line([(28, 424), (W - 28, 424)], fill=HAIR, width=1)
    center(d, (W / 2, 444), cap, F_CAP, col)
    return img


frames = [frame(i) for i in range(FRAMES)]
frames = [f.convert("P", palette=Image.ADAPTIVE, colors=64) for f in frames]
frames[0].save(OUT, save_all=True, append_images=frames[1:],
               duration=DURATION_MS, loop=0, optimize=True)
print("wrote", OUT)
