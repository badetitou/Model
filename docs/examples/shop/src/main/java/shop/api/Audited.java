package shop.api;

/** Marks a method whose calls are logged. */
public @interface Audited {
    String value() default "";
}
