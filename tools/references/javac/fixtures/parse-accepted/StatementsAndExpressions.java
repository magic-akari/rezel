import java.util.List;
import java.util.function.Function;
import java.util.function.IntFunction;
import static java.util.Objects.requireNonNull;

final class StatementsAndExpressions {
    static {
        System.getProperty("java.version");
    }

    {
        assert true : "initializer";
    }

    private int value;

    int run(Object input, List<String> values, AutoCloseable closeable) throws Exception {
        outer:
        for (int index = 0; index < values.size(); index++) {
            if (index == value) {
                continue outer;
            }
            value += index;
        }
        for (String item : values) {
            item.trim();
        }
        while (value < 3) {
            value++;
        }
        do {
            value--;
        } while (value > 0);

        try (closeable) {
            synchronized (this) {
                value += values.size();
            }
        } catch (RuntimeException | Error error) {
            throw error;
        } finally {
            value = 0;
        }

        return switch (input) {
            case String text when !text.isEmpty() -> text.length();
            case Integer number -> number.intValue();
            case null, default -> {
                yield value;
            }
        };
    }

    void statementSwitch(int input) {
        switch (input) {
            case 0:
            case 1:
                value++;
                break;
            default:
                return;
        }
    }

    Object expressions(int left, int right) {
        requireNonNull(this);
        int[] values = new int[] {1, 2};
        values[0] += left * right;
        int shifted = left << 1 | left >> 1 ^ left >>> 1 & right;
        boolean compared = left <= right && left != right || left == 0;
        Object selected = compared ? values[0] : shifted;
        Function<String, String> expressionLambda = text -> text.trim();
        Runnable statementLambda = () -> {
            value++;
        };
        Function<String, String> invokeReference = String::trim;
        IntFunction<String[]> constructorReference = String[]::new;
        Object anonymous =
                new Object() {
                    @Override
                    public String toString() {
                        return expressionLambda.apply("value");
                    }
                };
        statementLambda.run();
        return selected == null ? invokeReference.apply(anonymous.toString()) : selected;
    }

    double remainingOperators(int left, int right) {
        ;
        int prefix = ++left + --right;
        int unary = +left + -right + ~left;
        int arithmetic = left / right % 2 - prefix;
        boolean comparison = left >= right;
        left *= right;
        left /= right;
        left %= right;
        left -= right;
        left <<= 1;
        left >>= 1;
        left >>>= 1;
        left &= right;
        left ^= right;
        left |= right;
        float floatLiteral = 1.5f;
        double doubleLiteral = 0x1.0p2;
        char charLiteral = 'x';
        return comparison ? arithmetic + unary : floatLiteral + doubleLiteral + charLiteral;
    }
}
