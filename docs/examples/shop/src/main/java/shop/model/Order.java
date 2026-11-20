package shop.model;

import java.util.ArrayList;
import java.util.List;

import shop.api.Priced;

public class Order implements Priced {
    private final String id;
    private final List<Money> lines = new ArrayList<>();
    private Status status = Status.NEW;
    private int revisions;

    public Order(String id) {
        this.id = id;
    }

    public String id() {
        return id;
    }

    public void addLine(Money line) {
        lines.add(line);
        revisions += 1;
    }

    public void pay() {
        status = Status.PAID;
    }

    public Status status() {
        return status;
    }

    @Override
    public Money price() {
        Money sum = new Money(0, "EUR");
        for (Money line : lines) {
            sum = sum.plus(line);
        }
        return sum;
    }
}
