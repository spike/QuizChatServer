# [cfg(test)]

use my_project::websocket::{process_message, Message};

#[test]
fn test_process_message_with_text() {
	// Example test for processing text messages
	let message = Message::Text("Hello, world!".to_string());
	let result = process_message(message);
	assert_eq!(result, Some("Message processed: Hello, world!"));
}

#[test]
fn test_process_message_with_binary() {
	// Example test for processing binary messages
	let message = Message::Binary(vec![1, 2, 3, 4]);
	let result = process_message(message);
	assert_eq!(result, Some("Binary message processed"));
}

#[test]
fn test_process_message_with_close() {
	// Example test for processing close messages
	let message = Message::Close(None);
	let result = process_message(message);
	assert!(result.is_none());
}



