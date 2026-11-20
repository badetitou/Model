package shop.service;

public interface Repository<T> {
    T find(String id) throws NotFoundException;

    void save(T value);
}
