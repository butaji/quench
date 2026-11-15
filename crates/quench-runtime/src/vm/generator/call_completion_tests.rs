use super::*;

#[test]
fn generator_completion_moves_resume_state_once() {
    for reuses_result in [false, true] {
        let mut completion = CallCompletion::Generator(GeneratorResume { reuses_result });
        assert!(completion.take_promise().is_none());
        assert!(completion.is_generator());
        let resume = completion.take_generator().expect("generator completion");
        assert_eq!(resume.reuses_result, reuses_result);
        assert!(completion.take_generator().is_none());
        assert!(matches!(completion, CallCompletion::Value));
    }
}
