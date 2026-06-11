// Ink with React-style imports — demonstrates using import React from 'react'
// The compiler transforms this to work with the ink runtime

import React, {useState, useEffect} from 'react';
import {render, Text} from 'ink';

const Counter = () => {
    const [counter, setCounter] = useState(0);

    useEffect(() => {
        const timer = setInterval(() => {
            setCounter(previousCounter => previousCounter + 1);
        }, 100);

        return () => {
            clearInterval(timer);
        };
    }, []);

    return <Text color="green">{counter} tests passed</Text>;
};

render(<Counter />);
