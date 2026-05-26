// islands/CounterSimple.tsx - Simple working counter for testing

interface CounterProps {
    initial: number;
}

// A simple counter that works with the current parser limitations
export default function CounterSimple(props: CounterProps) {
    let count = props.initial;
    
    return (
        <div class="counter">
            <h2>Simple Counter</h2>
            <p>Count: {count}</p>
            <p>Initial: {props.initial}</p>
        </div>
    );
}
