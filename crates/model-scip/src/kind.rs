use model_core::EntityKind;
use scip::types::symbol_information::Kind;

/// Entity kind of a SCIP symbol kind; `None` for kinds the metamodel does
/// not model (parameters, literals, proof-assistant terms...).
pub fn entity_kind(kind: Kind) -> Option<EntityKind> {
    use EntityKind as E;
    use Kind as K;
    Some(match kind {
        K::Class | K::Object | K::SingletonClass => E::Class,
        K::Interface | K::Protocol => E::Interface,
        K::Trait | K::Mixin | K::TypeClass => E::Trait,
        K::Enum => E::Enum,
        K::Struct | K::Union => E::Struct,
        K::TypeAlias | K::Type | K::AssociatedType => E::TypeAlias,
        K::Method
        | K::AbstractMethod
        | K::StaticMethod
        | K::ProtocolMethod
        | K::PureVirtualMethod
        | K::TraitMethod
        | K::TypeClassMethod
        | K::SingletonMethod
        | K::MethodSpecification
        | K::MethodAlias
        | K::Getter
        | K::Setter
        | K::Accessor => E::Method,
        K::Constructor => E::Constructor,
        K::Function | K::Macro => E::Function,
        K::Field | K::StaticField | K::StaticDataMember => E::Field,
        K::Property | K::StaticProperty => E::Property,
        K::Constant | K::EnumMember => E::Constant,
        K::Variable | K::StaticVariable => E::Variable,
        K::Namespace | K::Package | K::PackageObject | K::Module | K::Library => E::Namespace,
        K::File => E::File,
        _ => return None,
    })
}
