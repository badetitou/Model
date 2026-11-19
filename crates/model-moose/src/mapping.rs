//! FM3 class → Model mapping, as data. Classes are matched by their simple
//! name (`Famix-Java-Entities.Method` → `Method`), so other Famix
//! languages that reuse the names map too.

use model_core::EntityKind;

/// Entity classes: simple FM3 name → archetype of the fine kind. The kind
/// is named after the class in `snake_case` (`famix_` prefixed when that is an
/// archetype's name: `famix_method`) and keeps the full FM3 name.
pub const ENTITIES: &[(&str, EntityKind)] = &[
    ("Package", EntityKind::Namespace),
    ("Namespace", EntityKind::Namespace),
    ("Module", EntityKind::Namespace),
    ("Class", EntityKind::Class),
    ("ParametricClass", EntityKind::Class),
    ("Exception", EntityKind::Class),
    ("Type", EntityKind::Class),
    ("PrimitiveType", EntityKind::Class),
    ("TypeParameter", EntityKind::Class),
    ("Wildcard", EntityKind::Class),
    ("Interface", EntityKind::Interface),
    ("ParametricInterface", EntityKind::Interface),
    ("AnnotationType", EntityKind::Interface),
    ("Enum", EntityKind::Enum),
    ("Trait", EntityKind::Trait),
    ("Method", EntityKind::Method),
    ("ParametricMethod", EntityKind::Method),
    ("Initializer", EntityKind::Method),
    ("AnnotationTypeAttribute", EntityKind::Method),
    ("Function", EntityKind::Function),
    ("Lambda", EntityKind::Function),
    ("Attribute", EntityKind::Field),
    ("EnumValue", EntityKind::Constant),
    ("Parameter", EntityKind::Variable),
    ("LocalVariable", EntityKind::Variable),
    ("ImplicitVariable", EntityKind::Variable),
    ("GlobalVariable", EntityKind::Variable),
];

/// How an association class becomes relations: for every `to` target,
/// `from` → `to` of kind `relation` (serialised name, see
/// `RelationKind::from_name`), at the association's source anchor.
pub struct AssociationDef {
    pub class: &'static str,
    pub relation: &'static str,
    pub from: &'static str,
    pub to: &'static str,
}

pub const ASSOCIATIONS: &[AssociationDef] = &[
    assoc("Invocation", "calls", "sender", "candidates"),
    assoc("ParametricInvocation", "calls", "sender", "candidates"),
    assoc("Access", "accesses", "accessor", "variable"),
    assoc("Reference", "references", "referencer", "referredEntity"),
    assoc(
        "ParametricReference",
        "references",
        "referencer",
        "referredEntity",
    ),
    assoc("Inheritance", "inherits", "subclass", "superclass"),
    assoc(
        "ParametricInheritance",
        "inherits",
        "subclass",
        "superclass",
    ),
    assoc(
        "Implementation",
        "implements",
        "implementingClass",
        "interface",
    ),
    assoc(
        "ParametricImplementation",
        "implements",
        "implementingClass",
        "interface",
    ),
    assoc("Import", "imports", "importingEntity", "importedEntity"),
];

const fn assoc(
    class: &'static str,
    relation: &'static str,
    from: &'static str,
    to: &'static str,
) -> AssociationDef {
    AssociationDef {
        class,
        relation,
        from,
        to,
    }
}

/// Typing associations: the typed entity points to one through `typing`.
pub const TYPINGS: &[&str] = &["EntityTyping", "ParametricEntityTyping"];

/// Properties of an entity that list the exceptions a method declares,
/// throws or catches: `throws` relations.
pub const THROWS_PROPS: &[&str] = &["declaredExceptions", "thrownExceptions", "caughtExceptions"];

/// Anchors: `element` → (`fileName`, `startPos`, `endPos`).
pub const ANCHORS: &[&str] = &["IndexedFileAnchor", "FileAnchor"];

/// Containment properties (`container` in FM3), best first when an entity
/// has several. Extended from an FM3 metamodel by
/// `ImportOptions::with_fm3`.
pub const CONTAINER_PROPS: &[&str] = &[
    "typeContainer",
    "parentType",
    "parentBehaviouralEntity",
    "parentEnum",
    "genericEntity",
    "parentAnnotationInstance",
    "annotationTypesContainer",
    "lambdaContainer",
    "parentPackage",
    "parentNamespace",
    "parentModule",
    "container",
];

/// Simple class name: the part after the last `.`.
pub fn simple_name(fm3: &str) -> &str {
    fm3.rsplit('.').next().unwrap_or(fm3)
}

/// `ParametricClass` → `parametric_class`.
pub fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

pub fn entity_archetype(simple: &str) -> Option<EntityKind> {
    ENTITIES.iter().find(|(n, _)| *n == simple).map(|(_, a)| *a)
}

pub fn association(simple: &str) -> Option<&'static AssociationDef> {
    ASSOCIATIONS.iter().find(|a| a.class == simple)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(simple_name("Famix-Java-Entities.Method"), "Method");
        assert_eq!(snake_case("ParametricClass"), "parametric_class");
        assert_eq!(entity_archetype("EnumValue"), Some(EntityKind::Constant));
    }
}
