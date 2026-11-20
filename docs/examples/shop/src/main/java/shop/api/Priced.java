package shop.api;

import shop.model.Money;

/** Anything that has a price. */
public interface Priced {
    Money price();
}
