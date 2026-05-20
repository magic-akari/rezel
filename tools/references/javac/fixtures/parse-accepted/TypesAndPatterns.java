import java.lang.annotation.ElementType;
import java.lang.annotation.Target;
import java.util.List;
import java.util.Map;

@Target({ElementType.TYPE_USE, ElementType.TYPE_PARAMETER})
@interface TypeUse {}

interface Left {}

interface Right {}

record Box<T>(T value) {}

final class TypesAndPatterns<@TypeUse T extends @TypeUse Left & @TypeUse Right> {
    byte byteValue;
    short shortValue;
    boolean booleanValue;
    char charValue;
    int intValue;
    long longValue;
    float floatValue;
    double doubleValue;
    Map.@TypeUse Entry<
                    @TypeUse ? extends @TypeUse Number,
                    @TypeUse ? super @TypeUse String>
            @TypeUse []
            @TypeUse []
            entries;
    List<?> unbounded;

    <R extends T> R @TypeUse [] method(
            final T @TypeUse [] input,
            String @TypeUse ... rest)
            throws ReflectiveOperationException {
        Object intersection = (@TypeUse T & @TypeUse Left) input[0];
        Object array = new @TypeUse String @TypeUse [1] @TypeUse [];
        Class<?> primitive = int.class;
        Class<?> primitiveArray = int[][].class;
        boolean binding = intersection instanceof @TypeUse String text;
        boolean deconstruction = intersection instanceof Box<String>(String nested);
        boolean unnamed = intersection instanceof Box<String>(_);
        return null;
    }
}
