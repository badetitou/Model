package shop.service;

import java.util.HashMap;
import java.util.Map;

import shop.api.Audited;
import shop.model.Order;

public class OrderService implements Repository<Order> {
    private final Map<String, Order> orders = new HashMap<>();

    @Override
    public void save(Order value) {
        orders.put(value.id(), value);
    }

    @Override
    @Audited("lookup")
    public Order find(String id) throws NotFoundException {
        Order found = orders.get(id);
        if (found == null) {
            throw new NotFoundException(id);
        }
        return found;
    }

    public long totalCents(String id) {
        try {
            Order order = find(id);
            return order.price().cents();
        } catch (NotFoundException e) {
            return retry(id, 1);
        }
    }

    private long retry(String id, int attempts) {
        return attempts > 3 ? 0 : totalCents(id);
    }

    private String unusedHelper(Order order) {
        return "order " + order;
    }
}
