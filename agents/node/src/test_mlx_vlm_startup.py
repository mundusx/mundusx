import unittest
from types import SimpleNamespace as NS

from mlx_vlm_startup import vision_runtime_proof, native_tools_runtime_proof


class VisionReadinessTests(unittest.TestCase):
    def model(self):
        text = NS(max_position_embeddings=1000000, rope_parameters={
            "type": "yarn", "factor": 4.0, "original_max_position_embeddings": 262144})
        return NS(vision_tower=object(), language_model=NS(args=text))

    def test_requires_real_encoder_and_processor(self):
        for model, processor in [(NS(), NS(image_processor=object())), (self.model(), NS())]:
            with self.assertRaises(ValueError):
                vision_runtime_proof(model, processor, 1000000, "selected-model")

    def test_rejects_context_that_was_not_applied(self):
        model = self.model()
        model.language_model.args.max_position_embeddings = 262144
        with self.assertRaises(ValueError):
            vision_runtime_proof(model, NS(image_processor=object()), 1000000, "selected-model")

    def test_proof_binds_loaded_model_and_context(self):
        proof = vision_runtime_proof(self.model(), NS(image_processor=object()), 1000000, "selected-model")
        self.assertEqual(proof["model"], "selected-model")
        self.assertEqual(proof["context_tokens"], 1000000)
        self.assertTrue(proof["supports_vision"])
        self.assertNotIn("native_tool_guard", proof)

    def test_tools_require_qualified_runtime_architecture_and_template(self):
        model = NS(config=NS(model_type="qwen3_5_moe"))
        processor = NS(tokenizer=NS(chat_template="{% if tools %}<tool_call>{% endif %}"))
        self.assertTrue(native_tools_runtime_proof(model, processor, "0.8.1"))
        self.assertFalse(native_tools_runtime_proof(model, processor, "0.8.0"))
        self.assertFalse(native_tools_runtime_proof(model, NS(chat_template="chat"), "0.8.1"))
        model.config.model_type = "unqualified_model"
        self.assertFalse(native_tools_runtime_proof(model, processor, "0.8.1"))


if __name__ == "__main__":
    unittest.main()
