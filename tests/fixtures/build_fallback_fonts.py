"""Rebuild the original, synthetic TASK-168 test fonts with fontTools (test tool only).

No downloaded outlines: each glyph is a rectangle with documented coordinates.
These fonts test coverage/placement, not real CJK typography or shaping.
"""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

ROOT = Path(__file__).parent

def build(name, mapping, rect, advance):
    font = FontBuilder(1000, isTTF=True)
    font.setupGlyphOrder(['.notdef', 'fixture', 'space'])
    font.setupCharacterMap({**{ord(ch): 'fixture' for ch in mapping}, 32: 'space', **({69: 'space', 66: 'space'} if name == 'fallback-primary' else {})})
    pen = TTGlyphPen(None)
    x0, y0, x1, y1 = rect
    pen.moveTo((x0, y0))
    pen.lineTo((x1, y0))
    pen.lineTo((x1, y1))
    pen.lineTo((x0, y1))
    pen.closePath()
    empty = TTGlyphPen(None).glyph()
    font.setupGlyf({'.notdef': empty, 'fixture': pen.glyph(), 'space': empty})
    font.setupHorizontalMetrics({'.notdef': (600, 0), 'fixture': (advance, x0), 'space': (600, 0)})
    ascent = {'fallback-primary': 800, 'fallback-cjk': 750, 'fallback-mark': 900}[name]
    font.setupHorizontalHeader(ascent=ascent, descent=ascent - 1000)
    family = name.replace('-', '')
    font.setupNameTable({'familyName': family, 'styleName': 'Regular', 'uniqueFontIdentifier': name,
                         'fullName': name, 'psName': name})
    font.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    font.setupPost()
    font.setupMaxp()
    font.font['head'].created = font.font['head'].modified = 2082844800
    font.save(ROOT / (name + '.ttf'))

build('fallback-primary', 'MAe', (100, 0, 500, 600), 600)
build('fallback-cjk', '繁體中𠮷E', (50, 0, 1150, 700), 1200)
build('fallback-mark', '\u0301', (-450, 650, -150, 750), 0)
