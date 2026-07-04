import Foundation

struct Greeting {
    let text: String

    func render(name: String) -> String {
        return text + name
    }
}
