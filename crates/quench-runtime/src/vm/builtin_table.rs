//! Which optional intrinsic families a realm installs (`RealmScope::Complete` installs all).
//!
//! An image knows its program before it runs: a family no record, atom or global names is not
//! installed, so its realm objects, install time and (through `BuiltinTable`'s `None` entries)
//! its installer code stay out of the image. `BuiltinTable::ALL` is every existing runtime's
//! table: `Vm::new` and `Runtime::new_static` install exactly what they did before.

use super::*;

/// The optional intrinsic families a realm installs. Each names the ES intrinsics whose code
/// and realm objects the family owns; the always-present core (`Object`, `Function`, `Error`,
/// `Array`, `String`, `Number`, `Symbol`, `console`) is no family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinFamilies(u32);

impl BuiltinFamilies {
    pub const REGEXP: Self = Self(1 << 0);
    pub const TYPED_MEMORY: Self = Self(1 << 1);
    pub const COLLECTIONS: Self = Self(1 << 2);
    pub const DATE: Self = Self(1 << 3);
    pub const BIGINT: Self = Self(1 << 4);
    pub const JSON: Self = Self(1 << 5);
    pub const PROMISE_ASYNC: Self = Self(1 << 6);
    pub const SHADOW_REALM: Self = Self(1 << 7);
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(
        Self::REGEXP.0 | Self::TYPED_MEMORY.0 | Self::COLLECTIONS.0 | Self::DATE.0
            | Self::BIGINT.0 | Self::JSON.0 | Self::PROMISE_ASYNC.0 | Self::SHADOW_REALM.0,
    );

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, family: Self) -> bool {
        self.0 & family.0 == family.0
    }

}

/// A realm installer: it defines the family's globals, constructors and prototypes.
pub type Installer = fn(&mut Vm, &ResidualProgram) -> Result<(), JsError>;

/// The family installers one realm runs. `None` keeps a family out of the realm and its code
/// out of the image; `ALL` is the complete realm every non-image runtime installs.
#[derive(Clone, Copy)]
pub struct BuiltinTable {
    pub families: BuiltinFamilies,
    pub regexp: Option<Installer>,
    pub typed_memory: Option<Installer>,
    pub collections: Option<Installer>,
    pub date: Option<Installer>,
    pub json: Option<Installer>,
    pub promise: Option<Installer>,
    pub shadow_realm: Option<Installer>,
}

impl BuiltinTable {
    pub const ALL: Self = Self {
        families: BuiltinFamilies::ALL,
        regexp: Some(install_regexp),
        typed_memory: Some(install_typed_memory),
        collections: Some(install_collections),
        date: Some(install_date),
        json: Some(install_json),
        promise: Some(install_promise),
        shadow_realm: Some(install_shadow_realm),
    };
}

fn install_regexp(vm: &mut Vm, program: &ResidualProgram) -> Result<(), JsError> {
    vm.install_regexp(program)
}

fn install_typed_memory(vm: &mut Vm, program: &ResidualProgram) -> Result<(), JsError> {
    vm.install_array_buffer(program)?;
    vm.install_typed_array(program)?;
    vm.install_lazy_builtin_namespace(program, "DataView", Native::DataViewInitialize)?;
    vm.install_lazy_builtin_namespace(program, "Atomics", Native::AtomicsInitialize)
}

fn install_collections(vm: &mut Vm, program: &ResidualProgram) -> Result<(), JsError> {
    vm.install_collections(program)?;
    vm.install_weak_collections(program)
}

fn install_date(vm: &mut Vm, program: &ResidualProgram) -> Result<(), JsError> {
    vm.install_date(program)
}

fn install_json(vm: &mut Vm, program: &ResidualProgram) -> Result<(), JsError> {
    let global = vm.realm.globals;
    vm.install_json_for_realm(program, global)
}

fn install_promise(vm: &mut Vm, program: &ResidualProgram) -> Result<(), JsError> {
    vm.install_promise(program)
}

fn install_shadow_realm(vm: &mut Vm, program: &ResidualProgram) -> Result<(), JsError> {
    vm.install_shadow_realm(program)
}

/// The installers the generated catalog names when its program reaches a family: naming one
/// is what links its code.
pub mod installers {
    use super::{install_collections, install_date, install_json, install_promise, install_regexp, install_shadow_realm, install_typed_memory, Installer};

    pub const REGEXP: Installer = install_regexp;
    pub const TYPED_MEMORY: Installer = install_typed_memory;
    pub const COLLECTIONS: Installer = install_collections;
    pub const DATE: Installer = install_date;
    pub const JSON: Installer = install_json;
    pub const PROMISE_ASYNC: Installer = install_promise;
    pub const SHADOW_REALM: Installer = install_shadow_realm;
}

impl Vm {
    /// The family's global and intrinsic symbols, after every family has installed: each
    /// family's post-installation links run only where it installed.
    pub(super) fn install_family_symbols(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        if self.builtins.collections.is_some() {
            let native_map = self.native_value(Native::Map);
            let native_set = self.native_value(Native::Set);
            self.install_collection_symbols(native_map, self.map_proto, native_set, self.set_proto, self.weak_map_proto, self.weak_set_proto)?;
        }
        if self.builtins.typed_memory.is_some() {
            self.install_typed_array_iterator_symbol(program)?;
            let native_typed_array = self.native_value(Native::TypedArray);
            self.install_typed_array_species(native_typed_array, self.typed_array_proto, self.realm.globals)?;
            self.install_array_buffer_species()?;
        }
        if self.builtins.regexp.is_some() {
            let native_reg_exp = self.native_value(Native::RegExp);
            self.install_regexp_symbol_properties(native_reg_exp, self.regexp_proto, self.realm.globals)?;
        }
        if self.builtins.shadow_realm.is_some() {
            self.install_shadow_realm_tag(self.shadow_realm_proto)?;
        }
        if self.builtins.date.is_some() {
            self.install_date_to_primitive(program)?;
        }
        Ok(())
    }
}
