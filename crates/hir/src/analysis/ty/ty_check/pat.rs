use std::ops::Range;

use crate::hir_def::{Partial, PatId, VariantKind, PatDescription, Pat};
use either::Either;

use super::{RecordLike, TyChecker, env::LocalBinding, path::RecordInitChecker};
use crate::analysis::{
    name_resolution::PathRes,
    ty::{
        binder::Binder,
        diagnostics::BodyDiag,
        ty_def::{self, InvalidCause, TyId},
        ty_lower::lower_hir_ty,
    },
};

impl<'db> TyChecker<'db> {
    /// Legacy wrapper that delegates to Pat::type_check.
    /// TODO: Migrate all call sites to use pat.type_check(tc, expected) directly.
    pub(super) fn check_pat(&mut self, pat: PatId, expected: TyId<'db>) -> TyId<'db> {
        self.body().wrap_pat(pat).type_check(self, expected)
    }

    fn check_record_pat_fields(&mut self, record_like: RecordLike<'db>, pat: PatId) {
        let Partial::Present(PatDescription::Record(_, fields)) = pat.data(self.db, self.body()) else {
            unreachable!()
        };

        let hir_db = self.db;
        let mut contains_rest = false;

        let pat_span = pat.span(self.body()).into_record_pat();
        let mut rec_checker = RecordInitChecker::new(self, &record_like);

        for (i, field_pat) in fields.iter().enumerate() {
            if field_pat.pat.is_rest(hir_db, rec_checker.tc.body()) {
                if contains_rest {
                    let diag = BodyDiag::DuplicatedRestPat(
                        field_pat.pat.span(rec_checker.tc.body()).into(),
                    );
                    rec_checker.tc.push_diag(diag);
                    continue;
                }

                contains_rest = true;
                continue;
            }

            let label = field_pat.label(hir_db, rec_checker.tc.body());
            let expected =
                match rec_checker.feed_label(label, pat_span.clone().fields().field(i).into()) {
                    Ok(ty) => ty,
                    Err(diag) => {
                        rec_checker.tc.push_diag(diag);
                        TyId::invalid(rec_checker.tc.db, InvalidCause::Other)
                    }
                };

            rec_checker.tc.check_pat(field_pat.pat, expected);
        }

        if let Err(diag) = rec_checker.finalize(pat_span.fields().into(), contains_rest) {
            self.push_diag(diag);
        }
    }

    fn unpack_rest_pat(
        &mut self,
        pat_tup: &[PatId],
        expected_len: Option<usize>,
    ) -> (Vec<TyId<'db>>, std::ops::Range<usize>) {
        let mut rest_start = None;
        for (i, &pat) in pat_tup.iter().enumerate() {
            if pat.is_rest(self.db, self.body()) && rest_start.replace(i).is_some() {
                let span = pat.span(self.body());
                self.push_diag(BodyDiag::DuplicatedRestPat(span.into()));
                return (
                    self.fresh_tys_n(expected_len.unwrap_or(0)),
                    Range::default(),
                );
            }
        }

        match rest_start {
            Some(rest_start) => {
                let expected_len = expected_len.unwrap_or(0);
                let minimum_len = pat_tup.len() - 1;

                if minimum_len <= expected_len {
                    let diff = expected_len - minimum_len;
                    let range = rest_start..rest_start + diff;
                    (self.fresh_tys_n(expected_len), range)
                } else {
                    (self.fresh_tys_n(minimum_len), Range::default())
                }
            }

            None => (self.fresh_tys_n(pat_tup.len()), Range::default()),
        }
    }
}

impl<'db> Pat<'db> {
    pub(super) fn type_check(self, tc: &mut TyChecker<'db>, expected: TyId<'db>) -> TyId<'db> {
        let Partial::Present(pat_data) = self.data(tc.db) else {
            let actual = TyId::invalid(tc.db, InvalidCause::ParseError);
            return tc.unify_ty(self.id(), actual, expected);
        };

        let ty = match pat_data {
            PatDescription::WildCard => {
                let ty_var = tc.table.new_var(ty_def::TyVarSort::General, &ty_def::Kind::Star);
                tc.unify_ty(self.id(), ty_var, expected)
            }
            PatDescription::Rest => expected,
            PatDescription::Lit(_) => self.type_check_lit(tc),
            PatDescription::Tuple(_) => self.type_check_tuple(tc, expected),
            PatDescription::Path(_, _) => self.type_check_path(tc),
            PatDescription::PathTuple(_, _) => self.type_check_path_tuple(tc),
            PatDescription::Record(_, _) => self.type_check_record(tc),
            PatDescription::Or(_, _) => self.type_check_or(tc, expected),
        };

        tc.unify_ty(self.id(), ty, expected)
    }

    fn type_check_lit(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(pat_data) = self.data(tc.db) else {
            unreachable!()
        };
        let PatDescription::Lit(lit) = pat_data else {
            unreachable!()
        };

        match lit {
            Partial::Present(lit) => tc.lit_ty(lit),
            Partial::Absent => TyId::invalid(tc.db, InvalidCause::ParseError),
        }
    }

    fn type_check_tuple(self, tc: &mut TyChecker<'db>, expected: TyId<'db>) -> TyId<'db> {
        let Partial::Present(pat_data) = self.data(tc.db) else {
            unreachable!()
        };
        let PatDescription::Tuple(pat_tup) = pat_data else {
            unreachable!()
        };

        let expected_len = match expected.decompose_ty_app(tc.db) {
            (base, args) if base.is_tuple(tc.db) => Some(args.len()),
            _ => None,
        };
        let (actual, rest_range) = tc.unpack_rest_pat(pat_tup, expected_len);
        let actual = TyId::tuple_with_elems(tc.db, &actual);

        let unified = tc.unify_ty(self.id(), actual, expected);
        if unified.has_invalid(tc.db) {
            // Even when unification fails, we need to check patterns to ensure
            // variable binding works correctly
            pat_tup.iter().for_each(|&pat| {
                tc.check_pat(pat, TyId::invalid(tc.db, InvalidCause::Other));
            });
            return unified;
        }

        let mut pat_idx = 0;
        for (i, &pat_ty) in unified.decompose_ty_app(tc.db).1.iter().enumerate() {
            if pat_idx >= pat_tup.len() {
                break;
            };

            if pat_tup[pat_idx].is_rest(tc.db, self.body()) {
                pat_idx += 1;
                continue;
            }

            if rest_range.contains(&i) {
                continue;
            }

            tc.check_pat(pat_tup[pat_idx], pat_ty);
            pat_idx += 1;
        }

        unified
    }

    fn type_check_path(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(pat_data) = self.data(tc.db) else {
            unreachable!()
        };
        let PatDescription::Path(path, is_mut) = pat_data else {
            unreachable!()
        };

        let Partial::Present(path) = path else {
            return TyId::invalid(tc.db, InvalidCause::ParseError);
        };

        let span = self.id().span(self.body()).into_path_pat();
        let res = tc.resolve_path(*path, true, span.clone().path());

        if path.is_bare_ident(tc.db) {
            match res {
                Ok(PathRes::Ty(ty) | PathRes::TyAlias(_, ty)) => {
                    let record_like = RecordLike::from_ty(ty);
                    if record_like.is_record(tc.db) {
                        let diag = BodyDiag::unit_variant_expected(
                            tc.db,
                            self.id().span(self.body()).into(),
                            record_like,
                        );
                        tc.push_diag(diag);
                        TyId::invalid(tc.db, InvalidCause::Other)
                    } else {
                        ty
                    }
                }
                Ok(PathRes::EnumVariant(variant)) => {
                    if matches!(variant.kind(tc.db), VariantKind::Unit) {
                        tc.table.instantiate_to_term(variant.ty)
                    } else {
                        let record_like = RecordLike::from_variant(variant);
                        let diag = BodyDiag::unit_variant_expected(
                            tc.db,
                            self.id().span(self.body()).into(),
                            record_like,
                        );

                        tc.push_diag(diag);
                        TyId::invalid(tc.db, InvalidCause::Other)
                    }
                }
                _ => {
                    let name = path.ident(tc.db).unwrap();
                    let binding = LocalBinding::local(self.id(), *is_mut);
                    if let Some(LocalBinding::Local {
                        pat: conflict_with, ..
                    }) = tc.env.register_pending_binding(name, binding)
                    {
                        let diag = BodyDiag::DuplicatedBinding {
                            primary: span.into(),
                            conflicat_with: conflict_with.span(self.body()).into(),
                            name,
                        };
                        tc.push_diag(diag);
                    }
                    tc.fresh_ty()
                }
            }
        } else {
            match res {
                Ok(
                    PathRes::Ty(ty)
                    | PathRes::TyAlias(_, ty)
                    | PathRes::Func(ty)
                    | PathRes::Const(_, ty),
                ) => {
                    let record_like = RecordLike::from_ty(ty);
                    if record_like.is_record(tc.db) {
                        let diag = BodyDiag::unit_variant_expected(
                            tc.db,
                            self.id().span(self.body()).into(),
                            record_like,
                        );
                        tc.push_diag(diag);
                        TyId::invalid(tc.db, InvalidCause::Other)
                    } else {
                        ty
                    }
                }
                Ok(PathRes::Trait(trait_)) => {
                    let diag = BodyDiag::NotValue {
                        primary: span.into(),
                        given: Either::Left(trait_.def(tc.db).trait_(tc.db).into()),
                    };
                    tc.push_diag(diag);
                    TyId::invalid(tc.db, InvalidCause::Other)
                }
                Ok(PathRes::EnumVariant(variant)) => {
                    if matches!(variant.kind(tc.db), VariantKind::Unit) {
                        tc.table.instantiate_to_term(variant.ty)
                    } else {
                        let record_like = RecordLike::from_variant(variant);
                        let diag = BodyDiag::unit_variant_expected(
                            tc.db,
                            self.id().span(self.body()).into(),
                            record_like,
                        );

                        tc.push_diag(diag);
                        TyId::invalid(tc.db, InvalidCause::Other)
                    }
                }
                Ok(PathRes::Mod(scope_id)) => {
                    let diag = BodyDiag::NotValue {
                        primary: span.into(),
                        given: Either::Left(scope_id.item()),
                    };
                    tc.push_diag(diag);
                    TyId::invalid(tc.db, InvalidCause::Other)
                }
                Ok(PathRes::Method(..) | PathRes::FuncParam(..)) => {
                    // TODO: diagnostic?
                    TyId::invalid(tc.db, InvalidCause::Other)
                }

                Err(_) => TyId::invalid(tc.db, InvalidCause::Other),
            }
        }
    }

    fn type_check_path_tuple(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(pat_data) = self.data(tc.db) else {
            unreachable!()
        };
        let PatDescription::PathTuple(Partial::Present(path), elems) = pat_data else {
            return TyId::invalid(tc.db, InvalidCause::ParseError);
        };

        let span = self.id().span(self.body()).into_path_tuple_pat();

        let (variant, expected_elems) = match tc.resolve_path(*path, true, span.clone().path()) {
            Ok(res) => match res {
                PathRes::Ty(ty)
                | PathRes::TyAlias(_, ty)
                | PathRes::Func(ty)
                | PathRes::Const(_, ty) => {
                    let diag = BodyDiag::tuple_variant_expected(
                        tc.db,
                        self.id().span(self.body()).into(),
                        Some(RecordLike::Type(ty)),
                    );
                    tc.push_diag(diag);
                    return TyId::invalid(tc.db, InvalidCause::Other);
                }

                PathRes::Trait(trait_) => {
                    let diag = BodyDiag::NotValue {
                        primary: span.into(),
                        given: Either::Left(trait_.def(tc.db).trait_(tc.db).into()),
                    };
                    tc.push_diag(diag);
                    return TyId::invalid(tc.db, InvalidCause::Other);
                }
                PathRes::EnumVariant(variant) => match variant.kind(tc.db) {
                    VariantKind::Tuple(elems) => (variant, elems),
                    _ => {
                        let diag = BodyDiag::tuple_variant_expected(
                            tc.db,
                            self.id().span(self.body()).into(),
                            Some(RecordLike::Variant(variant)),
                        );
                        tc.push_diag(diag);
                        return TyId::invalid(tc.db, InvalidCause::Other);
                    }
                },

                PathRes::Mod(scope) => {
                    let diag = BodyDiag::NotValue {
                        primary: span.into(),
                        given: Either::Left(scope.item()),
                    };
                    tc.push_diag(diag);
                    return TyId::invalid(tc.db, InvalidCause::Other);
                }

                PathRes::Method(..) | PathRes::FuncParam(..) => {
                    let diag = BodyDiag::tuple_variant_expected(tc.db, span.into(), None);
                    tc.push_diag(diag);
                    return TyId::invalid(tc.db, InvalidCause::Other);
                }
            },
            Err(_) => return TyId::invalid(tc.db, InvalidCause::Other),
        };

        let expected_len = expected_elems.len(tc.db);

        let (actual_elems, rest_range) = tc.unpack_rest_pat(elems, Some(expected_len));
        if actual_elems.len() != expected_len {
            let diag = BodyDiag::MismatchedFieldCount {
                primary: self.id().span(self.body()).into(),
                expected: expected_len,
                given: actual_elems.len(),
            };

            tc.push_diag(diag);
            return variant.ty;
        };

        let mut arg_idx = 0;
        for (i, &hir_ty) in expected_elems.data(tc.db).iter().enumerate() {
            if arg_idx >= elems.len() {
                break;
            }

            let current_pat_id = elems[arg_idx];
            let elem_ty = match hir_ty.to_opt() {
                Some(ty) => {
                    let ty = lower_hir_ty(
                        tc.db,
                        ty,
                        variant.enum_(tc.db).scope(),
                        tc.env.assumptions(),
                    );
                    let instantiated =
                        Binder::bind(ty).instantiate(tc.db, variant.ty.generic_args(tc.db));
                    // Normalize the type to resolve associated types
                    tc.normalize_ty(instantiated)
                }
                _ => TyId::invalid(tc.db, InvalidCause::ParseError),
            };

            // Call check_pat for the current source pattern element (current_pat_id).
            // If current_pat_id is PatDescription::Rest, its type will be unified with elem_ty (the type of the variant field it starts covering).
            // If the current variant field 'i' is covered by rest_range (meaning '..' covers it),
            // but current_pat_id is *not* PatDescription::Rest, it means this current_pat_id is for a field *after* the '..'.
            // In that case, we only proceed to check_pat if 'i' is NOT in rest_range.
            if current_pat_id.is_rest(tc.db, self.body()) {
                // For rest patterns, use the variant's type
                tc.check_pat(current_pat_id, variant.ty);
                // The '..' pattern from the source is consumed.
                // Subsequent iterations of the outer loop will skip variant fields covered by `rest_range`.
                arg_idx += 1;
            } else if !rest_range.contains(&i) {
                // This is an explicit pattern from the source (not '..'),
                // and it corresponds to a variant field not covered by any '..'.
                tc.check_pat(current_pat_id, elem_ty);
                arg_idx += 1;
            }
            // If rest_range.contains(&i) and current_pat_id is not PatDescription::Rest,
            // it means this variant field `i` is covered by a `..` that has already been processed (or will be).
            // We do nothing for this `elem_ty` and `current_pat_id` pair, and `arg_idx` is not incremented,
            // allowing `current_pat_id` to be matched against a subsequent variant field.
        }

        variant.ty
    }

    fn type_check_record(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(pat_data) = self.data(tc.db) else {
            unreachable!()
        };
        let PatDescription::Record(Partial::Present(path), _) = pat_data else {
            return TyId::invalid(tc.db, InvalidCause::ParseError);
        };

        let span = self.id().span(self.body()).into_record_pat();

        match tc.resolve_path(*path, true, span.clone().path()) {
            Ok(reso) => match reso {
                PathRes::Ty(ty) | PathRes::TyAlias(_, ty)
                    if RecordLike::from_ty(ty).is_record(tc.db) =>
                {
                    tc.check_record_pat_fields(RecordLike::from_ty(ty), self.id());
                    ty
                }

                PathRes::Ty(ty)
                | PathRes::TyAlias(_, ty)
                | PathRes::Func(ty)
                | PathRes::Const(_, ty) => {
                    let diag = BodyDiag::record_expected(
                        tc.db,
                        self.id().span(self.body()).into(),
                        Some(RecordLike::Type(ty)),
                    );
                    tc.push_diag(diag);
                    TyId::invalid(tc.db, InvalidCause::Other)
                }

                PathRes::Trait(trait_) => {
                    let diag = BodyDiag::NotValue {
                        primary: span.into(),
                        given: Either::Left(trait_.def(tc.db).trait_(tc.db).into()),
                    };
                    tc.push_diag(diag);
                    TyId::invalid(tc.db, InvalidCause::Other)
                }

                PathRes::EnumVariant(variant) => {
                    let ty = variant.ty;
                    let record_like = RecordLike::from_variant(variant);
                    if record_like.is_record(tc.db) {
                        tc.check_record_pat_fields(record_like, self.id());
                    }
                    ty
                }

                PathRes::Mod(scope) => {
                    let diag = BodyDiag::NotValue {
                        primary: span.into(),
                        given: Either::Left(scope.item()),
                    };
                    tc.push_diag(diag);
                    TyId::invalid(tc.db, InvalidCause::Other)
                }

                PathRes::Method(..) | PathRes::FuncParam(..) => {
                    let diag =
                        BodyDiag::record_expected(tc.db, self.id().span(self.body()).into(), None);
                    tc.push_diag(diag);
                    TyId::invalid(tc.db, InvalidCause::Other)
                }
            },
            Err(_) => TyId::invalid(tc.db, InvalidCause::Other),
        }
    }

    fn type_check_or(self, tc: &mut TyChecker<'db>, expected: TyId<'db>) -> TyId<'db> {
        let Partial::Present(pat_data) = self.data(tc.db) else {
            unreachable!()
        };
        let PatDescription::Or(lhs, rhs) = pat_data else {
            unreachable!()
        };

        tc.check_pat(*lhs, expected);
        tc.check_pat(*rhs, expected);

        expected
    }
}
