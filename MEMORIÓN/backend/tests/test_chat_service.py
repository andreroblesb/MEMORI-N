from backend.services.ai.chat_service import ChatRequest, ChatService


class FakeChatModel:
    def create_chat_completion(self, **kwargs):
        assert kwargs["messages"][0]["role"] == "system"
        assert kwargs["messages"][-1] == {"role": "user", "content": "Hola"}
        return {"choices": [{"message": {"content": "¡Hola! ¿Cómo puedo ayudarte?"}}]}


def test_chat_service_returns_model_content() -> None:
    service = ChatService(FakeChatModel())
    response = service.complete(
        ChatRequest(messages=[{"role": "user", "content": "Hola"}])
    )
    assert response.content == "¡Hola! ¿Cómo puedo ayudarte?"
