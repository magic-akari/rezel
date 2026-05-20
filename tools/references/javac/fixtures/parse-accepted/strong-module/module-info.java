module sample.strong {
    requires static java.compiler;
    requires transitive java.logging;
    requires java.xml;
    exports sample.strong.api;
    opens sample.strong.internal to sample.reflect;
    uses sample.strong.spi.Plugin;
    provides sample.strong.spi.Plugin with sample.strong.internal.PluginImpl;
}
