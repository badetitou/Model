package shop.model;

/** An amount in cents. */
public record Money(long cents, String currency) {
    public Money plus(Money other) {
        return new Money(cents + other.cents(), currency);
    }
}
