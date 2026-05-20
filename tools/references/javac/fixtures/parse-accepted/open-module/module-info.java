@Deprecated
open module sample.open {
    requires static transitive java.compiler;
    requires java.logging;
    exports sample.open.api to sample.consumer, sample.tests;
    uses sample.open.spi.Plugin;
    provides sample.open.spi.Plugin with sample.open.internal.PluginImpl;
}
