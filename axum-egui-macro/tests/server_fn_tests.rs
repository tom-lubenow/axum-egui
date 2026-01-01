//! Integration tests for the #[server] macro.

use axum_egui_macro::server;

// Test basic server function with arguments
mod test_with_args {
    use super::*;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct ServerFnError(String);

    impl std::fmt::Display for ServerFnError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[server]
    pub async fn add(a: i32, b: i32) -> Result<i32, ServerFnError> {
        Ok(a + b)
    }

    #[test]
    fn generates_request_struct() {
        // The macro should generate AddRequest struct
        let req = AddRequest { a: 1, b: 2 };
        assert_eq!(req.a, 1);
        assert_eq!(req.b, 2);
    }

    #[tokio::test]
    async fn function_works() {
        let result = add(2, 3).await;
        assert_eq!(result.unwrap(), 5);
    }
}

// Test server function without arguments
mod test_no_args {
    use super::*;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct ServerFnError(String);

    impl std::fmt::Display for ServerFnError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[server]
    pub async fn get_value() -> Result<i32, ServerFnError> {
        Ok(42)
    }

    #[tokio::test]
    async fn function_works() {
        let result = get_value().await;
        assert_eq!(result.unwrap(), 42);
    }
}

// Test server function with custom endpoint
mod test_custom_endpoint {
    use super::*;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct ServerFnError(String);

    impl std::fmt::Display for ServerFnError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[server(endpoint = "/api/v2/greet")]
    pub async fn greet(name: String) -> Result<String, ServerFnError> {
        Ok(format!("Hello, {}!", name))
    }

    #[tokio::test]
    async fn function_works() {
        let result = greet("World".into()).await;
        assert_eq!(result.unwrap(), "Hello, World!");
    }
}

// Test server function with complex return type
mod test_complex_return {
    use super::*;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct ServerFnError(String);

    impl std::fmt::Display for ServerFnError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct UserInfo {
        pub name: String,
        pub age: u32,
    }

    #[server]
    pub async fn get_user(id: u32) -> Result<UserInfo, ServerFnError> {
        Ok(UserInfo {
            name: format!("User{}", id),
            age: 25 + id,
        })
    }

    #[tokio::test]
    async fn function_works() {
        let result = get_user(1).await.unwrap();
        assert_eq!(result.name, "User1");
        assert_eq!(result.age, 26);
    }
}
