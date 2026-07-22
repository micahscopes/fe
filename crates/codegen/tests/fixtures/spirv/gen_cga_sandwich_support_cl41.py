#!/usr/bin/env python3
"""Generate/check a support-specialized runtime Cl(4,1) CGA sandwich."""

import argparse
import re
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUTPUT = HERE / "cga_sandwich_support_cl41.fe"
DEPTH = 5
LEAVES = 1 << DEPTH
METRIC = (1, 1, 1, 1, -1)
SPHERE_SUPPORT = (1, 2, 8, 16)
POINT_SUPPORT = (1, 2, 4, 8, 16)
VECTOR_SUPPORT = POINT_SUPPORT


def path(index, depth=DEPTH):
    return ".".join(
        "b" if index & (1 << bit) else "a"
        for bit in range(depth - 1, -1, -1)
    )


def tree(expressions):
    if len(expressions) == 1:
        return f"ScF {{ s: {expressions[0]} }}"
    half = len(expressions) // 2
    return f"NdF {{ a: {tree(expressions[:half])}, b: {tree(expressions[half:])} }}"


class Poly:
    """Exact polynomial oracle plus an operation-ordered structural expression."""

    def __init__(self, terms=(), code=None):
        self.terms = {m: c for m, c in terms if c}
        self.code = code if self.terms else None

    @classmethod
    def var(cls, name):
        return cls([((name,), Fraction(1))], name)

    @classmethod
    def constant(cls, value):
        value = Fraction(value)
        if not value:
            return cls()
        code = f"{value.numerator}.0" if value.denominator == 1 else f"{float(value):.8g}"
        return cls([((), value)], code)

    def __add__(self, other):
        terms = dict(self.terms)
        for monomial, coefficient in other.terms.items():
            terms[monomial] = terms.get(monomial, Fraction(0)) + coefficient
            if not terms[monomial]:
                del terms[monomial]
        if not terms:
            return Poly()
        if not self.terms:
            return Poly(terms.items(), other.emit())
        if not other.terms:
            return Poly(terms.items(), self.emit())
        return Poly(terms.items(), f"({self.emit()} + {other.emit()})")

    def __neg__(self):
        return Poly(((m, -c) for m, c in self.terms.items()), f"(-{self.emit()})")

    def __sub__(self, other):
        return self + -other

    def __mul__(self, other):
        terms = {}
        for left, left_coefficient in self.terms.items():
            for right, right_coefficient in other.terms.items():
                monomial = tuple(sorted(left + right))
                terms[monomial] = terms.get(monomial, Fraction(0)) + left_coefficient * right_coefficient
        if not terms:
            return Poly()
        return Poly(terms.items(), f"({self.emit()} * {other.emit()})")

    def scale(self, value):
        value = Fraction(value)
        if not value or not self.terms:
            return Poly()
        if value == 1:
            return self
        if value == -1:
            return -self
        scalar = Poly.constant(value)
        return Poly(
            ((m, c * value) for m, c in self.terms.items()),
            f"({scalar.emit()} * {self.emit()})",
        )

    def __eq__(self, other):
        return isinstance(other, Poly) and self.terms == other.terms

    def emit(self):
        if not self.terms:
            return "zero"
        return self.code


ZERO = Poly()


def add_vectors(left, right):
    return [a + b for a, b in zip(left, right)]


def neg_vector(value):
    return [-coefficient for coefficient in value]


def invol_recursive(value):
    """Grade involution by the same a+b*e_n structural recurrence as G-L1."""
    if len(value) == 1:
        return value
    half = len(value) // 2
    return invol_recursive(value[:half]) + neg_vector(invol_recursive(value[half:]))


def gp_recursive(left, right, metric=METRIC):
    """Fuchs-Thery pair-tree GP: one structural definition at every depth."""
    if len(left) == 1:
        return [left[0] * right[0]]
    half = len(left) // 2
    a1, b1 = left[:half], left[half:]
    a2, b2 = right[:half], right[half:]
    aa = gp_recursive(a1, a2, metric[:-1])
    bhb = gp_recursive(b1, invol_recursive(b2), metric[:-1])
    ab = gp_recursive(a1, b2, metric[:-1])
    bha = gp_recursive(b1, invol_recursive(a2), metric[:-1])
    low = add_vectors(aa, bhb if metric[-1] > 0 else neg_vector(bhb))
    high = add_vectors(ab, bha)
    return low + high


def cayley_blade(a, b):
    """Independent flat bitset/sign/metric product used as generator oracle."""
    negative = False
    for bit in range(DEPTH):
        if a & (1 << bit):
            negative ^= (b & ((1 << bit) - 1)).bit_count() % 2 == 1
        if a & b & (1 << bit):
            negative ^= METRIC[bit] < 0
    return a ^ b, -1 if negative else 1


def gp_cayley(left, right):
    out = [ZERO for _ in range(LEAVES)]
    for a, left_coefficient in enumerate(left):
        if not left_coefficient.terms:
            continue
        for b, right_coefficient in enumerate(right):
            if not right_coefficient.terms:
                continue
            blade, sign = cayley_blade(a, b)
            out[blade] = out[blade] + (left_coefficient * right_coefficient).scale(sign)
    return out


def operands():
    point = [ZERO for _ in range(LEAVES)]
    for index in POINT_SUPPORT:
        point[index] = Poly.var(f"p{index}")
    sphere = [ZERO for _ in range(LEAVES)]
    for index in SPHERE_SUPPORT:
        sphere[index] = Poly.var(f"s{index}")
    return sphere, point


def derive():
    sphere, point = operands()
    first = gp_recursive(sphere, point)
    sandwich = gp_recursive(first, sphere)
    assert first == gp_cayley(sphere, point)
    assert sandwich == gp_cayley(first, sphere)
    assert tuple(i for i, coefficient in enumerate(sphere) if coefficient.terms) == SPHERE_SUPPORT
    assert tuple(i for i, coefficient in enumerate(point) if coefficient.terms) == POINT_SUPPORT
    assert tuple(i for i, coefficient in enumerate(sandwich) if coefficient.terms) == VECTOR_SUPPORT
    return first, sandwich


def emit_helper_expression(polynomial):
    expression = polynomial.emit()
    replacements = {
        **{f"p{i}": f"point.{path(i)}.s" for i in POINT_SUPPORT},
        **{f"s{i}": f"sphere.{path(i)}.s" for i in SPHERE_SUPPORT},
    }
    return re.sub(r"\b(?:" + "|".join(replacements) + r")\b", lambda m: replacements[m.group()], expression)


def assert_structure(generated, first, sandwich, point_tree, sphere_tree, output_tree):
    assert DEPTH == 5 and LEAVES == 32 and METRIC == (1, 1, 1, 1, -1)
    assert len([coefficient for coefficient in first if coefficient.terms]) > len(SPHERE_SUPPORT)
    assert generated.count("let point: MvTF<5>") == 1
    assert generated.count("let sphere: MvTF<5>") == 1
    assert generated.count("fn sandwich_support_cl41(sphere: MvTF<5>, point: MvTF<5>) -> MvTF<5>") == 1
    assert generated.count("let sandwich: MvTF<5> = sandwich_support_cl41(sphere, point)") == 1
    assert point_tree in generated and sphere_tree in generated and output_tree in generated
    assert generated.count("let raw_") == len(VECTOR_SUPPORT)
    assert generated.count("__bitcast(__i32_from_f32(selected * 256.0))") == 1
    assert "let weight: f32 = raw_16 - raw_8" in generated
    for name in ("qx", "qy", "qz"):
        assert generated.count(f"let {name}: f32") == 1
    assert "scalar inversion identity" not in generated.lower()
    for index in VECTOR_SUPPORT:
        assert emit_helper_expression(sandwich[index]) in generated


def generate():
    first, sandwich = derive()
    point_values = {
        1: "x",
        2: "y",
        4: "z",
        8: "point_e4",
        16: "point_e5",
    }
    sphere_values = {1: "inv_cx", 2: "inv_cy", 8: "sphere_e4", 16: "sphere_e5"}
    point_tree = tree([point_values.get(i, "zero") for i in range(LEAVES)])
    sphere_tree = tree([sphere_values.get(i, "zero") for i in range(LEAVES)])
    output_tree = tree(
        [emit_helper_expression(sandwich[i]) if i in VECTOR_SUPPORT else "zero" for i in range(LEAVES)]
    )
    raw = "\n".join(
        f"    let raw_{index}: f32 = sandwich.{path(index)}.s" for index in VECTOR_SUPPORT
    )
    generated = f'''// @generated by gen_cga_sandwich_support_cl41.py; do not edit.
// Support-specialized Cl(4,1) recursive/Cayley sandwich S*P*S.
// S support is {{1,2,8,16}} for a runtime unit sphere center (inv_cx,inv_cy,0).
// P support is {{1,2,4,8,16}}. DFS leaf index is the blade bitset.
// The generator derives both products through the Fuchs-Thery pair-tree fold
// and independently proves every resulting polynomial with a flat Cayley oracle.

extern {{
    fn __i32_from_f32(_: f32) -> i32
    const fn __bitcast<From, To>(_: From) -> To
}}

pub struct ScF {{ pub s: f32 }}
pub struct NdF<A> {{ pub a: A, pub b: A }}
impl Copy for ScF {{}}
impl<A: Copy> Copy for NdF<A> {{}}

recursive type fn MvTF<const N: usize>() -> (*) {{
    match N {{
        0 => ScF
        _ => NdF<MvTF<{{N - 1}}>>
    }}
}}

#[inline(always)]
fn sandwich_support_cl41(sphere: MvTF<5>, point: MvTF<5>) -> MvTF<5> {{
    // Unsupported output blades are rebuilt from the typed point's zero leaf.
    let zero: f32 = point.a.a.a.a.a.s
    {output_tree}
}}

pub fn cga_sandwich_support_cl41(
    px: i32,
    py: i32,
    x: f32,
    y: f32,
    z: f32,
    inv_cx: f32,
    inv_cy: f32,
) -> u32 {{
    let zero: f32 = x - x
    let radius2: f32 = x * x + y * y + z * z
    let center2: f32 = inv_cx * inv_cx + inv_cy * inv_cy
    let point_e4: f32 = (radius2 - 1.0) * 0.5
    let point_e5: f32 = (radius2 + 1.0) * 0.5
    let sphere_e4: f32 = center2 * 0.5 - 1.0
    let sphere_e5: f32 = center2 * 0.5
    let point: MvTF<5> = {point_tree}
    let sphere: MvTF<5> = {sphere_tree}
    let sandwich: MvTF<5> = sandwich_support_cl41(sphere, point)
{raw}
    // A homogeneous conformal point has weight e5-e4. Normalize its vector
    // coefficients to recover Euclidean coordinates without changing the GP.
    let weight: f32 = raw_16 - raw_8
    let qx: f32 = raw_1 / weight
    let qy: f32 = raw_2 / weight
    let qz: f32 = raw_4 / weight
    let k: i32 = px + py * 2
    let selected: f32 = if k < 2 {{ if k < 1 {{ qx }} else {{ qy }} }} else {{ if k < 3 {{ qz }} else {{ weight }} }}
    __bitcast(__i32_from_f32(selected * 256.0))
}}
'''
    assert_structure(generated, first, sandwich, point_tree, sphere_tree, output_tree)
    return generated


def stats():
    first, sandwich = derive()
    return (
        f"depth={DEPTH} metric=+,+,+,+,- sphere_support={SPHERE_SUPPORT} "
        f"point_support={POINT_SUPPORT} first_support={sum(bool(v.terms) for v in first)} "
        f"sandwich_support={tuple(i for i,v in enumerate(sandwich) if v.terms)} "
        f"raw_polynomial_terms={sum(len(sandwich[i].terms) for i in VECTOR_SUPPORT)} "
        "runtime_f32_inputs=5 outputs=qx,qy,qz,weight observation=f32*256->i32->u32"
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--stdout", action="store_true")
    parser.add_argument("--stats", action="store_true")
    args = parser.parse_args()
    expected = generate()
    if args.stdout:
        print(expected, end="")
    elif args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != expected:
            raise SystemExit(f"stale generated fixture: run {Path(__file__).name}")
        print(f"{OUTPUT.name} is current")
    else:
        OUTPUT.write_text(expected)
        print(f"wrote {OUTPUT}")
    if args.stats:
        print(stats())


if __name__ == "__main__":
    main()
