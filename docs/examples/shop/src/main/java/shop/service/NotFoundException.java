package shop.service;

public class NotFoundException extends Exception {
    public NotFoundException(String id) {
        super("not found: " + id);
    }
}
