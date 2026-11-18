//! RegExp literal sites: the object each evaluation makes, or the one object a receiver-only
//! site shares while it cannot escape.
use super::*;

impl<H: Host> Vm<H> {
    pub(in crate::vm) fn regexp_literal(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site_index: usize,
    ) -> Result<Value, JsError> {
        self.regexp_literal_of(p, self.frames[frame].program, site_index)
    }

    /// The new RegExp object regular expression literal site `site_index` of `p`, the
    /// residual of `program`, evaluates to.
    pub(in crate::vm) fn regexp_literal_of(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        site_index: usize,
    ) -> Result<Value, JsError> {
        let site = p.regexp_literal_sites.get(site_index)
            .ok_or_else(|| JsError::validation("RegExp literal site is outside program".into()))?;
        if site.receiver {
            return self.regexp_literal_receiver(p, program, site_index);
        }
        match self.regexp_from_cached_site(program, site_index) {
            Some(object) => Ok(object),
            None => self.regexp_literal_fresh(p, program, site_index),
        }
    }

    /// The new object of literal site `site_index` made from the site's pattern and flags,
    /// whose record the site keeps for its later evaluations.
    pub(in crate::vm) fn regexp_literal_fresh(&mut self, p: &ResidualProgram, program: ProgramId, site_index: usize) -> Result<Value, JsError> {
        let site = p.regexp_literal_sites.get(site_index)
            .ok_or_else(|| JsError::validation("RegExp literal site is outside program".into()))?;
        let source = self.programs.constant(program, site.pattern_constant as usize)
            .and_then(|value| self.heap.string(value).cloned())
            .ok_or_else(|| JsError::validation("RegExp literal pattern is not a string".into()))?;
        let flags_text = self.programs.constant(program, site.flags_constant as usize)
            .filter(|value| self.heap.string(*value).is_some())
            .ok_or_else(|| JsError::validation("RegExp literal flags are not a string".into()))?;
        let (prototype, constructor) = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .map(|intrinsics| (intrinsics.prototype, intrinsics.constructor))
            .expect("RegExp intrinsics are installed for the active realm");
        let owner = crate::heap::RegExpLegacyOwner::Enabled(constructor);
        let meta = match self.programs.regexp_literal_meta(program, site_index) {
            Some(meta) => meta,
            None => self.regexp_literal_site_meta(program, site_index, (source, flags_text), owner)?,
        };
        // A site's record names the realm it was first evaluated in; another realm's
        // evaluation owns a record of its own.
        let meta = if meta.legacy_constructor.constructor() == constructor {
            meta
        } else {
            Rc::new(crate::heap::RegExpMeta { legacy_constructor: owner, ..(*meta).clone() })
        };
        self.regexp_from_meta(prototype, meta)
    }

    /// The record of literal site `site_index` of `program`, made from its `source` and flags
    /// text and kept for the site's later evaluations.
    fn regexp_literal_site_meta(
        &mut self,
        program: ProgramId,
        site_index: usize,
        (source, flags_text): (JsString, Value),
        owner: crate::heap::RegExpLegacyOwner,
    ) -> Result<Rc<crate::heap::RegExpMeta>, JsError> {
        let flags = self.heap.string(flags_text).expect("checked above").host_string();
        // The compiler checked the literal's pattern and flags (early errors), so its
        // matcher compiles on the first match (`Vm::regexp_matcher`), as a snapshot's
        // patterns do: a module evaluates every literal it holds, and most never run
        // (tr46's 34 Unicode classes were 3.3% of the whatwg-url npm scenario).
        let meta = Rc::new(crate::heap::RegExpMeta {
            source,
            flags: RegExpFlags::validated(flags),
            legacy_constructor: owner,
            matcher: crate::heap::RegExpMatcher::default(),
        });
        if !self.programs.cache_regexp_literal_meta(program, site_index, Rc::clone(&meta)) {
            return Err(JsError::validation("RegExp literal cache site is invalid".into()));
        }
        Ok(meta)
    }

    /// The object of receiver-only literal site `site_index` (`RegExpLiteralSite::receiver`):
    /// one per site while `RegExp.prototype.test` and `exec` are the intrinsics and never
    /// changed (`Fuses::ever_changed`), so only the builtins ever see it, and its flags hold
    /// neither `g` nor `y`, so they never write its `lastIndex`. Each evaluation is then a
    /// fresh object in every observable way; any other evaluation makes a fresh one.
    fn regexp_literal_receiver(&mut self, p: &ResidualProgram, program: ProgramId, site_index: usize) -> Result<Value, JsError> {
        let reusable = self.regexp_builtins_never_replaced();
        if reusable && let Some(object) = self.programs.regexp_literal_receiver(program, site_index) {
            return Ok(object);
        }
        let object = match self.regexp_from_cached_site(program, site_index) {
            Some(object) => object,
            None => self.regexp_literal_fresh(p, program, site_index)?,
        };
        let stateless = matches!(self.heap.get(object), Some(Cell::RegExp { meta, .. })
            if !meta.flags.contains(RegExpFlags::GLOBAL) && !meta.flags.contains(RegExpFlags::STICKY));
        if reusable && stateless {
            let fuses = self.heap.dependencies_mut().fuses_mut();
            fuses.guard(crate::code_dependencies::IntactFuse::RegExpTest);
            fuses.guard(crate::code_dependencies::IntactFuse::RegExpExec);
            self.programs.cache_regexp_literal_receiver(program, site_index, object);
        }
        Ok(object)
    }

    /// Whether `RegExp.prototype.test` and `exec` are this realm's intrinsics and their
    /// holder never changed them since they were proved.
    fn regexp_builtins_never_replaced(&mut self) -> bool {
        use crate::code_dependencies::IntactFuse;
        let fuses = [IntactFuse::RegExpTest, IntactFuse::RegExpExec];
        fuses.into_iter().all(|fuse| self.fuse_intact(fuse)) && !fuses.into_iter().any(|fuse| self.heap.dependencies().fuses().ever_changed(fuse))
    }
}
