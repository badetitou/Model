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

/// GraphQL concept and root field names of an FM3 class, derived from its
/// name alone: `Famix-Java-Entities.Record` → (`JavaRecord`, `javaRecords`).
/// The second segment of the package name (`Java`) prefixes the class name, so
/// languages never clash with each other or with the archetype types; a
/// package without that shape uses the bare class name, prefixed `Famix`
/// when it would shadow an archetype. `None` when no valid name results.
pub fn concept_names(fm3_class: &str, simple: &str) -> Option<(String, String)> {
    let package = fm3_class.rsplit_once('.').map_or("", |(p, _)| p);
    let mut segments = package.split('-');
    let lang = match (segments.next(), segments.next(), segments.next()) {
        (Some(_), Some(l), Some(_)) => l,
        _ => "",
    };
    let clean = |s: &str| -> String { s.chars().filter(char::is_ascii_alphanumeric).collect() };
    let mut ty = format!("{}{}", clean(lang), clean(simple));
    if ty.is_empty() || ty.starts_with(|c: char| !c.is_ascii_uppercase()) {
        return None;
    }
    if lang.is_empty()
        && model_core::EntityKind::ALL
            .iter()
            .any(|a| a.as_str().replace('_', "").eq_ignore_ascii_case(&ty))
    {
        ty = format!("Famix{ty}");
    }
    let mut root = ty.clone();
    root[..1].make_ascii_lowercase();
    Some((ty, plural(&root)))
}

fn plural(word: &str) -> String {
    let consonant_y = word.strip_suffix('y').filter(|stem| {
        stem.chars()
            .last()
            .is_some_and(|c| !"aeiou".contains(c.to_ascii_lowercase()))
    });
    if let Some(stem) = consonant_y {
        format!("{stem}ies")
    } else if word.ends_with(['s', 'x', 'z']) || word.ends_with("ch") || word.ends_with("sh") {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
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

// ---------------------------------------------------------------- export

/// Package of the FamixJava classes the exporter writes.
pub const JAVA_PACKAGE: &str = "Famix-Java-Entities";

/// Kind name → simple FM3 class, for kinds with no `fm3` of their own
/// (the ones tree-sitter and SCIP define). Archetype kinds are named after
/// the archetype (`method`, `field`...). A kind absent here and without an
/// `fm3` is not exported (files, functions, traits, plain variables...).
pub const EXPORT_KINDS: &[(&str, &str)] = &[
    ("namespace", "Package"),
    ("class", "Class"),
    ("record", "Class"),
    ("interface", "Interface"),
    ("annotation_type", "AnnotationType"),
    ("enum", "Enum"),
    ("method", "Method"),
    ("constructor", "Method"),
    ("field", "Attribute"),
    ("constant", "Attribute"),
    ("enum_constant", "EnumValue"),
    ("parameter", "Parameter"),
    ("local_variable", "LocalVariable"),
    ("type_parameter", "TypeParameter"),
    ("annotation", "AnnotationInstance"),
    ("comment", "Comment"),
];

/// The class an entity becomes when it owns type parameters
/// (`typeParameters` is derived: the owner's class tells).
pub const PARAMETRIC: &[(&str, &str)] = &[
    ("Class", "ParametricClass"),
    ("Interface", "ParametricInterface"),
    ("Method", "ParametricMethod"),
];

/// Containment on export: the child class (simple name) is written with
/// `property` = parent when the parent's class is in `parents` (empty =
/// any). First matching row wins; this is where the (child, parent) pair
/// picks the property: `AnnotationType` hangs from a package through
/// `annotationTypesContainer`, but from a type through `typeContainer`.
pub struct ContainerRule {
    pub child: &'static str,
    pub parents: &'static [&'static str],
    pub property: &'static str,
}

const fn rule(
    child: &'static str,
    parents: &'static [&'static str],
    property: &'static str,
) -> ContainerRule {
    ContainerRule {
        child,
        parents,
        property,
    }
}

/// Classes that own methods and attributes (`TWithMethods`).
const TYPES: &[&str] = &[
    "Class",
    "ParametricClass",
    "Exception",
    "Interface",
    "ParametricInterface",
    "Enum",
    "AnnotationType",
];

const METHODS: &[&str] = &["Method", "ParametricMethod"];

pub const EXPORT_CONTAINERS: &[ContainerRule] = &[
    rule("Package", &["Package"], "parentPackage"),
    rule("AnnotationType", &["Package"], "annotationTypesContainer"),
    rule("AnnotationType", &[], "typeContainer"),
    rule("Class", &[], "typeContainer"),
    rule("ParametricClass", &[], "typeContainer"),
    rule("Exception", &[], "typeContainer"),
    rule("Interface", &[], "typeContainer"),
    rule("ParametricInterface", &[], "typeContainer"),
    rule("Enum", &[], "typeContainer"),
    rule("PrimitiveType", &[], "typeContainer"),
    rule("TypeParameter", &[], "typeContainer"),
    rule("Method", TYPES, "parentType"),
    rule("ParametricMethod", TYPES, "parentType"),
    rule("Attribute", TYPES, "parentType"),
    rule("AnnotationTypeAttribute", TYPES, "parentType"),
    rule("EnumValue", &["Enum"], "parentEnum"),
    rule("Parameter", METHODS, "parentBehaviouralEntity"),
    rule("LocalVariable", METHODS, "parentBehaviouralEntity"),
    rule("ImplicitVariable", METHODS, "parentBehaviouralEntity"),
    rule("Comment", &[], "commentedEntity"),
    rule("AnnotationInstance", &[], "annotatedEntity"),
];

/// Classes whose instances can be commented (`TWithComments`) or annotated
/// are many; a comment whose parent is none of these (a file) is dropped.
pub const COMMENTABLE: &[&str] = &[
    "Package",
    "Class",
    "ParametricClass",
    "Exception",
    "Interface",
    "ParametricInterface",
    "Enum",
    "AnnotationType",
    "Method",
    "ParametricMethod",
    "Attribute",
    "EnumValue",
    "Parameter",
    "LocalVariable",
    "AnnotationTypeAttribute",
];

/// Container property for `child` under `parent`, if any. A parent that
/// does not fit (a method of an anonymous class sits under a method in
/// Model, but Famix wants a type) is skipped by the exporter in favour of
/// the nearest ancestor that does.
pub fn export_container(child: &str, parent: &str) -> Option<&'static str> {
    EXPORT_CONTAINERS
        .iter()
        .find(|r| r.child == child && (r.parents.is_empty() || r.parents.contains(&parent)))
        .map(|r| r.property)
}

/// Stored primitive properties of Famix, by property name: the classes that
/// own it. A `moose.<name>` prop of an entity (set by the importer) is
/// written back when its class is listed. `visibility` is read as is.
/// Derived properties and metrics are never written.
pub const PRIMITIVE_PROPS: &[(&str, &[&str])] = &[
    (
        "visibility",
        &[
            "Class",
            "ParametricClass",
            "Exception",
            "Interface",
            "ParametricInterface",
            "Enum",
            "AnnotationType",
            "Method",
            "ParametricMethod",
            "Attribute",
        ],
    ),
    (
        "isAbstract",
        &[
            "Class",
            "ParametricClass",
            "Exception",
            "Method",
            "ParametricMethod",
        ],
    ),
    (
        "isFinal",
        &[
            "Class",
            "ParametricClass",
            "Exception",
            "Interface",
            "ParametricInterface",
            "Method",
            "ParametricMethod",
            "Attribute",
            "Parameter",
            "LocalVariable",
        ],
    ),
    (
        "isClassSide",
        &[
            "Class",
            "ParametricClass",
            "Exception",
            "Interface",
            "ParametricInterface",
            "Method",
            "ParametricMethod",
            "Attribute",
        ],
    ),
    ("isSynchronized", &["Method", "ParametricMethod"]),
    ("isVolatile", &["Attribute"]),
    ("isTransient", &["Attribute"]),
];

#[cfg(test)]
mod export_tests {
    use super::*;

    #[test]
    fn container_depends_on_the_parent() {
        assert_eq!(
            export_container("AnnotationType", "Package"),
            Some("annotationTypesContainer")
        );
        assert_eq!(
            export_container("AnnotationType", "Class"),
            Some("typeContainer")
        );
        assert_eq!(export_container("Class", "Package"), Some("typeContainer"));
        assert_eq!(export_container("Nothing", "Package"), None);
    }
}

/// Classes Moose accepts at the end of an association (checked against the
/// slots of the FamixJava classes of a Moose image: a `Reference` from a
/// class, for instance, is refused by `FamixJavaReference>>referencer:`).
pub const TYPE_CLASSES: &[&str] = &[
    "Class",
    "ParametricClass",
    "Exception",
    "Interface",
    "ParametricInterface",
    "Enum",
    "AnnotationType",
    "PrimitiveType",
    "TypeParameter",
];

/// `TTypedEntity` and `TAccessible`: what has a `typing` / can be accessed.
pub const VARIABLE_CLASSES: &[&str] = &[
    "Attribute",
    "EnumValue",
    "ImplicitVariable",
    "LocalVariable",
    "Parameter",
    "AnnotationTypeAttribute",
];

/// Throwables: what `declaredExceptions` may list.
pub const THROWABLE_CLASSES: &[&str] = &["Class", "ParametricClass", "Exception", "TypeParameter"];

/// Types that inherit (`TWithInheritances`).
pub const INHERITING_CLASSES: &[&str] = &[
    "Class",
    "ParametricClass",
    "Exception",
    "Interface",
    "ParametricInterface",
    "Enum",
    "AnnotationType",
];

/// Entities with imports (`TImportable` / `TWithImports`).
pub const IMPORT_CLASSES: &[&str] = &[
    "Package",
    "Class",
    "ParametricClass",
    "Exception",
    "Interface",
    "ParametricInterface",
    "Enum",
    "Method",
    "ParametricMethod",
];

pub fn is_method(class: &str) -> bool {
    METHODS.contains(&class)
}

#[cfg(test)]
mod concept_tests {
    use super::concept_names;

    #[test]
    fn names_come_from_the_fm3_class() {
        let n = |c: &str, s: &str| concept_names(c, s);
        assert_eq!(
            n("Famix-Java-Entities.Record", "Record"),
            Some(("JavaRecord".into(), "javaRecords".into()))
        );
        assert_eq!(
            n("Famix-Java-Entities.Class", "Class"),
            Some(("JavaClass".into(), "javaClasses".into()))
        );
        assert_eq!(
            n("Famix-Java-Entities.Annotation", "Annotation").map(|x| x.1),
            Some("javaAnnotations".into())
        );
        assert_eq!(
            n("Famix-Java-Entities.Property", "Property").map(|x| x.1),
            Some("javaProperties".into())
        );
        // No language segment: bare name, prefixed when it shadows an archetype.
        assert_eq!(
            n("Pkg.Widget", "Widget"),
            Some(("Widget".into(), "widgets".into()))
        );
        assert_eq!(
            n("Pkg.Method", "Method"),
            Some(("FamixMethod".into(), "famixMethods".into()))
        );
        assert_eq!(n("Pkg.9", "9"), None);
    }
}
