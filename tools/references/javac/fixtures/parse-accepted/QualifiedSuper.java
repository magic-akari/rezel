import java.util.function.Supplier;

interface QualifiedSuperBase {
    default String describe() {
        return "base";
    }
}

final class QualifiedSuper implements QualifiedSuperBase {
    Supplier<String> reference() {
        return QualifiedSuperBase.super::describe;
    }

    String invocation() {
        return QualifiedSuperBase.super.describe();
    }
}
